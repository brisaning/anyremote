//! Gate del ciclo C1 (RF-01): sesión real ARSP entre dos nodos sobre TLS loopback,
//! con permisos explícitos, métricas de enlace y cierre limpio.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyremote_identity::Identity;
use anyremote_session::{LinkStats, SessionEvent, SessionState};
use anyremote_transport::arsp::{self, proto};
use anyremote_transport::{client_config, generate_loopback_cert, server_config};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[tokio::test]
async fn e2e_session_establishment_with_metrics() {
    let cert = generate_loopback_cert().expect("cert");
    let srv_cfg = server_config(&cert).expect("srv cfg");
    let cli_cfg = client_config(&cert.cert).expect("cli cfg");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();

    let controlled_id = Identity::generate();
    let controller_id = Identity::generate();
    let controller_id_str = controller_id.node_id().to_string();
    let controlled_id_str = controlled_id.node_id().to_string();

    // ---- Rol controlado (lado servidor) ----
    let controlled = tokio::spawn(async move {
        let acceptor = TlsAcceptor::from(Arc::new(srv_cfg));
        let (tcp, _) = listener.accept().await.expect("accept");
        let mut ch = arsp::Channel::new(acceptor.accept(tcp).await.expect("tls"));

        let mut state = SessionState::Handshaking;
        let hello = ch.recv().await.expect("hello");
        arsp::check_version(&hello).expect("version soportada");
        assert_eq!(
            arsp::announced_node_id(&hello).as_deref(),
            Some(controller_id_str.as_str())
        );

        ch.send(arsp::welcome(&controlled_id_str))
            .await
            .expect("welcome");
        state = state.transition(SessionEvent::HandshakeOk).unwrap();
        assert_eq!(state, SessionState::AwaitingPermission);

        ch.send(proto::envelope::Payload::PermissionRequest(
            proto::PermissionRequest {
                permissions: vec!["keyboard".into(), "mouse".into()],
            },
        ))
        .await
        .expect("perm req");
        let resp = ch.recv().await.expect("perm resp");
        let granted = match resp.payload {
            Some(proto::envelope::Payload::PermissionResponse(r)) => r,
            other => panic!("payload inesperado: {other:?}"),
        };
        assert!(granted.granted);
        assert_eq!(granted.permissions, vec!["keyboard", "mouse"]);
        state = state.transition(SessionEvent::PermissionGranted).unwrap();
        assert_eq!(state, SessionState::Active);

        // Eco de heartbeats (4 rondas) y cierre ordenado.
        for _ in 0..4 {
            let hb = ch.recv().await.expect("heartbeat");
            match hb.payload {
                Some(proto::envelope::Payload::Heartbeat(h)) => {
                    assert!(!h.echo, "el controlado debe recibir heartbeats sin eco");
                    ch.send(proto::envelope::Payload::Heartbeat(proto::Heartbeat {
                        seq: h.seq,
                        sent_unix_ms: h.sent_unix_ms,
                        echo: true,
                    }))
                    .await
                    .expect("eco");
                }
                other => panic!("payload inesperado: {other:?}"),
            }
        }

        let bye = ch.recv().await.expect("bye");
        assert!(matches!(
            bye.payload,
            Some(proto::envelope::Payload::Bye(_))
        ));
        state = state.transition(SessionEvent::StartClose).unwrap();
        state = state.transition(SessionEvent::Closed).unwrap();
        assert_eq!(state, SessionState::Closed);
    });

    // ---- Rol controlador (lado cliente) ----
    let tcp = TcpStream::connect(addr).await.expect("connect");
    let sni = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let mut ch = arsp::Channel::new(
        TlsConnector::from(Arc::new(cli_cfg))
            .connect(sni, tcp)
            .await
            .expect("tls client"),
    );

    let mut state = SessionState::Handshaking;
    ch.send(arsp::hello(controller_id.node_id().as_str(), "c1-demo"))
        .await
        .expect("hello");
    let welcome = ch.recv().await.expect("welcome");
    arsp::check_version(&welcome).expect("version soportada");
    assert_eq!(
        arsp::announced_node_id(&welcome).as_deref(),
        Some(controlled_id.node_id().as_str())
    );
    state = state.transition(SessionEvent::HandshakeOk).unwrap();

    let req = ch.recv().await.expect("perm req");
    assert!(matches!(
        req.payload,
        Some(proto::envelope::Payload::PermissionRequest(_))
    ));
    ch.send(proto::envelope::Payload::PermissionResponse(
        proto::PermissionResponse {
            granted: true,
            permissions: vec!["keyboard".into(), "mouse".into()],
        },
    ))
    .await
    .expect("grant");
    state = state.transition(SessionEvent::PermissionGranted).unwrap();
    assert_eq!(state, SessionState::Active);

    // ---- Métricas básicas: 4 heartbeats con RTT ----
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
        let echo = ch.recv().await.expect("hb echo");
        match echo.payload {
            Some(proto::envelope::Payload::Heartbeat(h)) => {
                assert!(h.echo && h.seq == i);
                let rtt = (now_ms() - h.sent_unix_ms).max(0) as u64;
                stats.record_rtt(rtt);
            }
            other => panic!("payload inesperado: {other:?}"),
        }
    }
    assert_eq!(stats.heartbeats_ok, 4);
    assert!(stats.min_rtt_ms.unwrap() <= stats.last_rtt_ms.unwrap());

    ch.send(proto::envelope::Payload::Bye(proto::Bye {
        reason: "fin de sesión de prueba".into(),
    }))
    .await
    .expect("bye");
    state = state.transition(SessionEvent::StartClose).unwrap();
    assert_eq!(
        state.transition(SessionEvent::Closed).unwrap(),
        SessionState::Closed
    );

    controlled.await.expect("tarea controlado");
}
