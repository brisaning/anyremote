//! Demo del ciclo C1 (RF-01): sesión ARSP real entre dos nodos sobre TLS 1.3 —
//! establecimiento, concesión de permisos, latidos con RTT y cierre ordenado.
//! Ejecutar: `cargo run -p anyremote-transport --example session_demo`
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyremote_identity::{Identity, NodeId, TrustStore};
use anyremote_session::{LinkStats, SessionEvent, SessionState};
use anyremote_transport::arsp::{self, proto};
use anyremote_transport::auth;
use anyremote_transport::{client_config, generate_loopback_cert, server_config};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[tokio::main]
async fn main() {
    println!("=== anyremote C1 — sesión ARSP entre dos nodos (RF-01) ===");
    let controller = Identity::generate();
    let controlled = Identity::generate();
    println!(
        "[ids] controlador={} controlado={}",
        controller.node_id(),
        controlled.node_id()
    );

    let cert = generate_loopback_cert().expect("cert");
    let srv_cfg = server_config(&cert).expect("cfg srv");
    let cli_cfg = client_config(&cert.cert).expect("cfg cli");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();

    let ctrl_id_for_task = controller.node_id().to_string();
    let controller_pk = controller.verifying_key_bytes();
    let controller_id_task = ctrl_id_for_task.clone();
    let controlled_id_task = controlled.node_id().to_string();
    let controlled_task = tokio::spawn(async move {
        let acceptor = TlsAcceptor::from(Arc::new(srv_cfg));
        let (tcp, peer_addr) = listener.accept().await.expect("accept");
        println!("[controlado] conexión TLS de {peer_addr}");
        let tls = acceptor.accept(tcp).await.expect("tls");
        let binding = auth::binding_of_server(&tls).expect("binding");
        let mut ch = arsp::Channel::new(tls);

        // C2: autenticación mutua con channel binding + trust store (H-RD-02/H-RD-04).
        let mut store = TrustStore::new();
        store.trust(NodeId::parse(&controller_id_task).unwrap(), controller_pk);
        let mut gate = auth::AuthGate::new();
        let peer = auth::recv_validate_auth(&mut ch, &mut gate, &store, &binding)
            .await
            .expect("auth del controlador");
        println!(
            "[controlado] controlador autenticado: {} (dispositivo confiable={})",
            peer.node_id, peer.trusted
        );
        auth::send_auth(&mut ch, &controlled, &binding)
            .await
            .expect("auth propio");

        let mut state = SessionState::Handshaking;
        let hello = ch.recv().await.expect("hello");
        arsp::check_version(&hello).expect("versión");
        let hint = match &hello.payload {
            Some(proto::envelope::Payload::Hello(h)) => h.session_hint.clone(),
            _ => String::new(),
        };
        println!(
            "[controlado] Hello ARSP de {} (hint={hint:?})",
            arsp::announced_node_id(&hello).unwrap_or_default()
        );
        ch.send(arsp::welcome(&controlled_id_task)).await.ok();
        state = state.transition(SessionEvent::HandshakeOk).unwrap();
        println!("[controlado] Welcome enviado — sesión en AwaitingPermission");

        ch.send(proto::envelope::Payload::PermissionRequest(
            proto::PermissionRequest {
                permissions: vec!["keyboard".into(), "mouse".into()],
            },
        ))
        .await
        .expect("perm");
        println!("[controlado] solicitados permisos: keyboard, mouse");
        let resp = ch.recv().await.expect("resp");
        if let Some(proto::envelope::Payload::PermissionResponse(r)) = resp.payload {
            println!(
                "[controlado] respuesta: granted={} perms={:?}",
                r.granted, r.permissions
            );
            state = state.transition(SessionEvent::PermissionGranted).unwrap();
        }

        for _ in 0..4 {
            if let Some(proto::envelope::Payload::Heartbeat(h)) =
                ch.recv().await.expect("hb").payload
            {
                ch.send(proto::envelope::Payload::Heartbeat(proto::Heartbeat {
                    seq: h.seq,
                    sent_unix_ms: h.sent_unix_ms,
                    echo: true,
                }))
                .await
                .ok();
            }
        }
        if let Some(proto::envelope::Payload::Bye(b)) = ch.recv().await.expect("bye").payload {
            println!("[controlado] Bye recibido: {:?}", b.reason);
        }
        state = state.transition(SessionEvent::StartClose).unwrap();
        state.transition(SessionEvent::Closed).unwrap()
    });

    let tcp = TcpStream::connect(addr).await.expect("connect");
    let sni = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let tls = TlsConnector::from(Arc::new(cli_cfg))
        .connect(sni, tcp)
        .await
        .expect("cliente tls");
    let binding = auth::binding_of_client(&tls).expect("binding");
    let mut ch = arsp::Channel::new(tls);

    // C2: autenticación mutua desde el controlador.
    auth::send_auth(&mut ch, &controller, &binding)
        .await
        .expect("auth");
    let mut gate = auth::AuthGate::new();
    let store = TrustStore::new();
    let peer = auth::recv_validate_auth(&mut ch, &mut gate, &store, &binding)
        .await
        .expect("auth del controlado");
    println!(
        "[controlador] controlado verificado: {} (confiable={})",
        peer.node_id, peer.trusted
    );

    let mut state = SessionState::Handshaking;
    ch.send(arsp::hello(&controller.node_id().to_string(), "demo-c2"))
        .await
        .expect("hello");
    let welcome = ch.recv().await.expect("welcome");
    arsp::check_version(&welcome).expect("versión welcome");
    state = state.transition(SessionEvent::HandshakeOk).unwrap();
    println!(
        "[controlador] Welcome de {} — AwaitingPermission",
        arsp::announced_node_id(&welcome).unwrap_or_default()
    );

    let req = ch.recv().await.expect("req");
    if let Some(proto::envelope::Payload::PermissionRequest(r)) = req.payload {
        println!("[controlador] solicitado conceder: {:?}", r.permissions);
    }
    ch.send(proto::envelope::Payload::PermissionResponse(
        proto::PermissionResponse {
            granted: true,
            permissions: vec!["keyboard".into(), "mouse".into()],
        },
    ))
    .await
    .expect("grant");
    state = state.transition(SessionEvent::PermissionGranted).unwrap();
    println!("[controlador] permisos concedidos — sesión Active");

    let mut stats = LinkStats::default();
    for i in 1..=4u64 {
        let sent = now_ms();
        ch.send(proto::envelope::Payload::Heartbeat(proto::Heartbeat {
            seq: i,
            sent_unix_ms: sent,
            echo: false,
        }))
        .await
        .expect("hb");
        if let Some(proto::envelope::Payload::Heartbeat(h)) = ch.recv().await.expect("echo").payload
        {
            let rtt = (now_ms() - h.sent_unix_ms).max(0) as u64;
            stats.record_rtt(rtt);
            println!("[métricas] heartbeat #{i} rtt={rtt} ms");
        }
    }
    println!(
        "[métricas] heartbeats_ok={} rtt_min={} rtt_ult={}",
        stats.heartbeats_ok,
        stats.min_rtt_ms.unwrap_or(0),
        stats.last_rtt_ms.unwrap_or(0)
    );

    ch.send(proto::envelope::Payload::Bye(proto::Bye {
        reason: "demo completada".into(),
    }))
    .await
    .expect("bye");
    state = state.transition(SessionEvent::StartClose).unwrap();
    state.transition(SessionEvent::Closed).unwrap();
    println!("[controlador] sesión cerrada limpiamente (Closing -> Closed)");
    let final_state = controlled_task.await.expect("tarea controlado");
    println!("[controlado] estado final: {final_state:?}");
    println!("=== C1 completado: {ctrl_id_for_task} -> sesión ARSP operativa ===");
}
