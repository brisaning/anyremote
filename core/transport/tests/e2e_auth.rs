//! Gate del ciclo C2 (RF-03/RNF-01): autenticación mutua ed25519 con channel binding,
//! trust store de dispositivos y límite de intentos (patrones H-RD-02/H-RD-04).

use std::sync::Arc;

use anyremote_identity::{Identity, TrustStore};
use anyremote_transport::arsp::{self, proto};
use anyremote_transport::auth;
use anyremote_transport::{client_config, generate_loopback_cert, server_config, TransportError};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{client::TlsStream, TlsAcceptor, TlsConnector};

type Tls = TlsStream<TcpStream>;

async fn connect_channel(
    addr: std::net::SocketAddr,
    cfg: rustls::ClientConfig,
) -> (ChannelWrap, Vec<u8>) {
    let tcp = TcpStream::connect(addr).await.expect("connect");
    let sni = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let tls = TlsConnector::from(Arc::new(cfg))
        .connect(sni, tcp)
        .await
        .expect("tls client");
    let binding = auth::binding_of_client(&tls).expect("binding");
    (ChannelWrap(arsp::Channel::new(tls)), binding)
}

struct ChannelWrap(arsp::Channel<Tls>);

fn forged_auth(id: &Identity, binding: &[u8]) -> proto::envelope::Payload {
    proto::envelope::Payload::Auth(proto::Auth {
        node_id: id.node_id().to_string(),
        pubkey: id.verifying_key_bytes().to_vec(),
        signature: vec![0u8; 64], // firma inválida
        binding: binding.to_vec(),
    })
}

/// (1) Autenticación mutua feliz: el controlado confía en el controlador registrado;
/// el controlador valida al controlado como desconocido (trusted=false).
#[tokio::test]
async fn e2e_mutual_auth_with_trust_store() {
    let cert = generate_loopback_cert().expect("cert");
    let srv_cfg = server_config(&cert).expect("cfg");
    let cli_cfg = client_config(&cert.cert).expect("cfg");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();

    let controller = Identity::generate();
    let controlled = Identity::generate();
    let controller_id_str = controller.node_id().to_string();
    let controller_id_task = controller_id_str.clone();
    let controlled_id_str = controlled.node_id().to_string();

    let mut controlled_store = TrustStore::new();
    controlled_store.trust(controller.node_id(), controller.verifying_key_bytes());

    let controlled_task = tokio::spawn(async move {
        let acceptor = TlsAcceptor::from(Arc::new(srv_cfg));
        let (tcp, _) = listener.accept().await.expect("accept");
        let tls = acceptor.accept(tcp).await.expect("tls");
        let binding = auth::binding_of_server(&tls).expect("binding");
        let mut ch = arsp::Channel::new(tls);

        let mut gate = auth::AuthGate::new();
        let peer = auth::recv_validate_auth(&mut ch, &mut gate, &controlled_store, &binding)
            .await
            .expect("auth del controlador");
        assert_eq!(peer.node_id.to_string(), controller_id_task);
        assert!(peer.trusted, "el controlador registrado debe ser trusted");

        auth::send_auth(&mut ch, &controlled, &binding)
            .await
            .expect("enviar auth");
        peer.node_id.to_string()
    });

    let (mut ch, binding) = connect_channel(addr, cli_cfg).await;
    auth::send_auth(&mut ch.0, &controller, &binding)
        .await
        .expect("enviar auth");
    let mut gate = auth::AuthGate::new();
    let store = TrustStore::new(); // el controlador no tiene dispositivos previos
    let peer = auth::recv_validate_auth(&mut ch.0, &mut gate, &store, &binding)
        .await
        .expect("auth del controlado");
    assert_eq!(peer.node_id.to_string(), controlled_id_str);
    assert!(!peer.trusted, "controlado desconocido para el controlador");

    let server_view = controlled_task.await.expect("tarea controlado");
    assert_eq!(server_view, controller_id_str);
}

/// (2) Rechazo de prueba inválida: firma alterada y binding de canal ajeno.
#[tokio::test]
async fn auth_rejects_tampered_proof_and_foreign_binding() {
    for tamper_foreign_binding in [false, true] {
        let cert = generate_loopback_cert().expect("cert");
        let srv_cfg = server_config(&cert).expect("cfg");
        let cli_cfg = client_config(&cert.cert).expect("cfg");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().unwrap();
        let attacker = Identity::generate();

        let server = tokio::spawn(async move {
            let acceptor = TlsAcceptor::from(Arc::new(srv_cfg));
            let (tcp, _) = listener.accept().await.expect("accept");
            let tls = acceptor.accept(tcp).await.expect("tls");
            let binding = auth::binding_of_server(&tls).expect("binding");
            let mut ch = arsp::Channel::new(tls);
            let mut gate = auth::AuthGate::new();
            let store = TrustStore::new();
            auth::recv_validate_auth(&mut ch, &mut gate, &store, &binding).await
        });

        let (mut ch, binding) = connect_channel(addr, cli_cfg).await;
        let payload = if tamper_foreign_binding {
            // Firma válida pero sobre un binding ajeno al canal TLS.
            let foreign = b"binding-de-otro-canal-tls-diferente";
            proto::envelope::Payload::Auth(proto::Auth {
                node_id: attacker.node_id().to_string(),
                pubkey: attacker.verifying_key_bytes().to_vec(),
                signature: attacker.sign_auth(foreign).to_vec(),
                binding: binding.clone(),
            })
        } else {
            forged_auth(&attacker, &binding)
        };
        ch.0.send(payload).await.expect("enviar prueba falsa");

        let err = server
            .await
            .expect("tarea servidor")
            .expect_err("debe rechazar");
        assert!(
            matches!(err, TransportError::Auth(_)),
            "error inesperado: {err:?}"
        );
    }
}

/// (3) Límite de intentos (H-RD-04):3 fallos en la misma conexión cortan la sesión.
#[tokio::test]
async fn auth_gate_limits_attempts() {
    let cert = generate_loopback_cert().expect("cert");
    let srv_cfg = server_config(&cert).expect("cfg");
    let cli_cfg = client_config(&cert.cert).expect("cfg");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let acceptor = TlsAcceptor::from(Arc::new(srv_cfg));
        let (tcp, _) = listener.accept().await.expect("accept");
        let tls = acceptor.accept(tcp).await.expect("tls");
        let binding = auth::binding_of_server(&tls).expect("binding");
        let mut ch = arsp::Channel::new(tls);
        let mut gate = auth::AuthGate::new();
        let store = TrustStore::new();

        let mut last = None;
        for _ in 0..auth::MAX_AUTH_FAILURES {
            match auth::recv_validate_auth(&mut ch, &mut gate, &store, &binding).await {
                Ok(_) => panic!("no debe autenticar"),
                Err(e) => last = Some(e),
            }
        }
        last.unwrap()
    });

    let (mut ch, binding) = connect_channel(addr, cli_cfg).await;
    let attacker = Identity::generate();
    for _ in 0..auth::MAX_AUTH_FAILURES {
        ch.0.send(forged_auth(&attacker, &binding))
            .await
            .expect("enviar");
    }

    let err = server.await.expect("tarea servidor");
    match err {
        TransportError::Auth(msg) => assert!(
            msg.contains("demasiados intentos"),
            "se esperaba corte por límite, llegó: {msg}"
        ),
        other => panic!("error inesperado: {other:?}"),
    }
}
