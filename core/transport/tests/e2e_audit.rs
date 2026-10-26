//! Gate del ciclo C7 (RF-05): eventos de auditoría por sesión sobre una sesión viva
//! + informe de diagnóstico accionable (JSONL verificable línea a línea).

use std::sync::Arc;

use anyremote_filexfer::{sha256_hex, FileReceiver, FileSender, TransferPolicy};
use anyremote_session::LinkStats;
use anyremote_telemetry::{AuditEventKind, SessionLog, SessionReport};
use anyremote_transport::arsp::{self, proto};
use anyremote_transport::{client_config, generate_loopback_cert, server_config};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

const SESSION: &str = "ses-audit-1";

#[tokio::test]
async fn e2e_audit_events_and_diagnostic_report() {
    let cert = generate_loopback_cert().expect("cert");
    let srv_cfg = server_config(&cert).expect("cfg");
    let cli_cfg = client_config(&cert.cert).expect("cfg");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();

    // ---- Controlado: rol servidor (welcome, eco de latidos, archivo, bye) ----
    let controlled = tokio::spawn(async move {
        let acceptor = TlsAcceptor::from(Arc::new(srv_cfg));
        let (tcp, _) = listener.accept().await.expect("accept");
        let mut ch = arsp::Channel::new(acceptor.accept(tcp).await.expect("tls"));

        let hello = ch.recv().await.expect("hello");
        arsp::check_version(&hello).expect("versión");
        ch.send(arsp::welcome("nodo-srv")).await.expect("welcome");

        for _ in 0..4 {
            let hb = ch.recv().await.expect("hb");
            if let Some(proto::envelope::Payload::Heartbeat(h)) = hb.payload {
                ch.send(proto::envelope::Payload::Heartbeat(proto::Heartbeat {
                    seq: h.seq,
                    sent_unix_ms: h.sent_unix_ms,
                    echo: true,
                }))
                .await
                .expect("eco");
            } else {
                panic!("esperaba heartbeat");
            }
        }

        let offer = ch.recv().await.expect("offer");
        if let Some(proto::envelope::Payload::FileOffer(o)) = offer.payload {
            let meta = arsp::file_offer_from_wire(o).expect("meta");
            let size = meta.size;
            let mut recv = FileReceiver::new(TransferPolicy::default());
            recv.accept_offer(meta).expect("aceptar");
            while recv.next_offset(9).expect("conocido") < size {
                let chunk = ch.recv().await.expect("chunk");
                if let Some(proto::envelope::Payload::FileChunk(c)) = chunk.payload {
                    recv.write_chunk(c.transfer_id, c.offset, &c.data).expect("chunk");
                } else {
                    panic!("esperaba FileChunk");
                }
            }
            let done = recv.finish(9).expect("hash");
            ch.send(proto::envelope::Payload::FileDone(arsp::file_done_to_wire(
                9,
                true,
                &sha256_hex(&done),
            )))
            .await
            .expect("done");
        }

        let bye = ch.recv().await.expect("bye");
        assert!(matches!(bye.payload, Some(proto::envelope::Payload::Bye(_))));
    });

    // ---- Controlador: ejecuta la sesión auditable ----
    let dir = std::env::temp_dir().join(format!("anyremote-c7-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut log = SessionLog::open(&dir).expect("log");
    let mut report = SessionReport::new(SESSION);

    log.record(
        AuditEventKind::SessionStarted,
        SESSION,
        serde_json::json!({"target": addr.to_string(), "profile": "manual"}),
    )
    .expect("ev1");

    let tcp = TcpStream::connect(addr).await.expect("connect");
    let sni = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let mut ch = arsp::Channel::new(
        TlsConnector::from(Arc::new(cli_cfg))
            .connect(sni, tcp)
            .await
            .expect("tls"),
    );

    ch.send(arsp::hello("nodo-cli", "ses-audit-1"))
        .await
        .expect("hello");
    let welcome = ch.recv().await.expect("welcome");
    arsp::check_version(&welcome).expect("versión welcome");
    log.record(
        AuditEventKind::SessionEstablished,
        SESSION,
        serde_json::json!({"peer": arsp::announced_node_id(&welcome)}),
    )
    .expect("ev2");

    // 4 latidos con métricas (RF-05 base C1).
    let mut stats = LinkStats::default();
    for i in 1..=4u64 {
        let sent = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        ch.send(proto::envelope::Payload::Heartbeat(proto::Heartbeat {
            seq: i,
            sent_unix_ms: sent,
            echo: false,
        }))
        .await
        .expect("hb");
        let echo = ch.recv().await.expect("echo");
        if let Some(proto::envelope::Payload::Heartbeat(h)) = echo.payload {
            let rtt = (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
                - h.sent_unix_ms)
                .max(0) as u64;
            stats.record_rtt(rtt);
            report.record_heartbeat(rtt);
        }
    }
    log.record(
        AuditEventKind::HeartbeatSummary,
        SESSION,
        serde_json::json!({
            "heartbeats_ok": stats.heartbeats_ok,
            "rtt_min_ms": stats.min_rtt_ms,
            "rtt_last_ms": stats.last_rtt_ms,
        }),
    )
    .expect("ev3");

    // Transferencia de archivo real sobre la sesión → evento FileTransfer.
    let data = b"contenido auditado de prueba";
    let sender = FileSender::new(TransferPolicy::default());
    let meta = sender.offer(9, "audit.txt", data).expect("oferta");
    let digest = sha256_hex(data);
    ch.send(proto::envelope::Payload::FileOffer(arsp::file_offer_to_wire(&meta)))
        .await
        .expect("offer");
    for (off, chunk) in sender.chunks_from(data, 0, 8) {
        ch.send(proto::envelope::Payload::FileChunk(arsp::file_chunk_to_wire(
            9, off, chunk,
        )))
        .await
        .expect("chunk");
    }
    let done = ch.recv().await.expect("done");
    match done.payload {
        Some(proto::envelope::Payload::FileDone(d)) => {
            assert!(d.ok, "receptor confirmó: {}", d.detail);
            assert_eq!(d.detail, digest);
        }
        other => panic!("esperaba FileDone: {other:?}"),
    }
    report.record_file();
    report.record_frame_sent(meta.size as usize);
    log.record(
        AuditEventKind::FileTransfer,
        SESSION,
        serde_json::json!({"transfer_id": 9, "name": "audit.txt", "sha256": digest}),
    )
    .expect("ev4");

    ch.send(proto::envelope::Payload::Bye(proto::Bye {
        reason: "gate c7".into(),
    }))
    .await
    .expect("bye");
    report.finish();
    log.record(AuditEventKind::Disconnected, SESSION, report.disconnect_event())
        .expect("ev5");

    controlled.await.expect("tarea controlado");

    // ---- Verificación del log (diagnóstico accionable) ----
    let events = SessionLog::read_all(log.path()).expect("leer jsonl");
    let kinds: Vec<_> = events.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![
            AuditEventKind::SessionStarted,
            AuditEventKind::SessionEstablished,
            AuditEventKind::HeartbeatSummary,
            AuditEventKind::FileTransfer,
            AuditEventKind::Disconnected,
        ],
        "secuencia de auditoría por sesión"
    );
    for e in &events {
        assert_eq!(e.session_id, SESSION, "session_id coherente");
        assert!(e.ts_unix_ms > 0);
    }
    // Monotonía temporal.
    for w in events.windows(2) {
        assert!(w[0].ts_unix_ms <= w[1].ts_unix_ms);
    }
    // Diagnóstico accionable en el cierre.
    let last = events.last().unwrap();
    assert_eq!(last.detail["heartbeats_ok"], 4);
    assert!(last.detail["rtt_min_ms"].is_number());
    assert_eq!(last.detail["files_transferred"], 1);
    assert_eq!(last.detail["session_id"], SESSION);
    // Evento de transferencia con hash real.
    assert_eq!(events[3].detail["sha256"], digest.as_str());

    let _ = std::fs::remove_dir_all(&dir);
}
