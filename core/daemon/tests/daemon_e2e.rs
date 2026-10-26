//! Gate de integración: `anyremoted` listen + connect en proceso (loopback),
//! cableando TODOS los ciclos: cert-store+TLS (S0), auth mutua (C2), ARSP/
//! permisos (C1), portapapeles con semilla (C4), archivo con hash (C5),
//! auditoría JSONL (C7) y cierre por max-secs.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyremote_daemon::{run_connect, run_listen, ClipboardMode, DaemonConfig};

fn temp_cfg_dir(tag: &str) -> PathBuf {
    // Cert-store compartido: ambos roles usan el MISMO dir (v0.1 localhost).
    let d = std::env::temp_dir().join(format!("anyremote-integ-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("config dir");
    d
}

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind probe");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

#[tokio::test]
async fn daemon_listen_connect_full_integration() {
    let cfg_dir = temp_cfg_dir("full");
    let port = free_port().await;

    // Archivo a transferir (C5).
    let payload_dir = cfg_dir.join("payload");
    std::fs::create_dir_all(&payload_dir).expect("payload dir");
    let file_path = payload_dir.join("informe-integracion.bin");
    let bytes: Vec<u8> = (0..40_000u32).map(|i| (i % 253) as u8).collect();
    std::fs::write(&file_path, &bytes).expect("write payload");

    let seed = "semilla del controlador";

    // ---- Lado controlado (listen) ----
    let listen_cfg = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.bind = format!("127.0.0.1:{port}");
        c.clipboard = ClipboardMode::Memory;
        c.max_secs = 6;
        c
    };
    let listen = tokio::spawn(async move { run_listen(listen_cfg).await });

    // Pequeña espera a que el listener enlace.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // ---- Lado controlador (connect) con eventos capturados ----
    let events_ctl = Arc::new(Mutex::new(Vec::<String>::new()));
    let events_clone = events_ctl.clone();
    let connect_cfg = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Memory;
        c.clipboard_seed = Some(seed.to_string());
        c.send_file = Some(file_path.clone());
        c.max_secs = 5;
        c.on_event = Some(Box::new(move |line: String| {
            events_clone.lock().unwrap().push(line);
        }));
        c
    };
    let controller = tokio::spawn(async move { run_connect(connect_cfg).await });

    let ctl = controller.await.expect("join controller").expect("controller ok");
    let srv = listen.await.expect("join listener").expect("listener ok");

    // --- Verificación de eventos del controlador (flujo completo) ---
    let ev_text = events_ctl.lock().unwrap().join("\n");
    assert!(ev_text.contains("\"event\":\"connecting\""), "falta connecting:\n{ev_text}");
    assert!(ev_text.contains("\"event\":\"session_active\""), "falta session_active");
    assert!(ev_text.contains("\"role\":\"controller\""));
    assert!(ev_text.contains("\"event\":\"file_sent\""), "falta file_sent");
    assert!(ev_text.contains("\"event\":\"file_ack\""), "falta file_ack (hash confirmado)");
    assert!(ev_text.contains("\"event\":\"closed\""), "falta closed");

    // --- Sesión autenticada y permisos concedidos (C1/C2) ---
    assert!(ctl.report.heartbeats_ok >= 2, "latidos: {}", ctl.report.heartbeats_ok);
    assert!(ctl.report.rtt_min_ms.is_some());

    // --- Transferencia de archivos (C5) con hash confirmado por el receptor ---
    assert_eq!(ctl.report.files_transferred, 1, "ack de archivo en controlador");
    assert_eq!(srv.report.files_transferred, 1, "archivo recibido en controlado");
    let srv_events = srv.events.join("\n");
    assert!(srv_events.contains("\"event\":\"session_active\""));
    assert!(srv_events.contains("\"event\":\"file_received\""));
    assert!(srv_events.contains("\"role\":\"controlled\""));

    // --- Portapapeles cableado (C4): la semilla viajó controlador → controlado ---
    assert_eq!(
        srv.clipboard_final.as_deref(),
        Some(seed),
        "controlado debe tener la semilla aplicada"
    );
    assert_eq!(
        ctl.clipboard_final.as_deref(),
        Some(seed),
        "controlador conserva su texto local"
    );

    // --- Auditoría (C7): JSONL legible con eventos clave del lado servidor ---
    let audit_dir = cfg_dir.join("audit");
    let mut files = std::fs::read_dir(&audit_dir)
        .expect("audit dir")
        .flatten()
        .map(|e| e.path())
        .collect::<Vec<_>>();
    files.sort();
    assert!(!files.is_empty(), "debe haber al menos un log JSONL");
    let mut kinds = Vec::new();
    for f in &files {
        for ev in anyremote_telemetry::SessionLog::read_all(f).expect("jsonl válido") {
            kinds.push(format!("{:?}", ev.kind));
        }
    }
    let joined = kinds.join(",");
    assert!(joined.contains("SessionEstablished"), "audit: {joined}");
    assert!(joined.contains("PermissionGranted"), "audit: {joined}");
    assert!(joined.contains("FileTransfer"), "audit: {joined}");
    assert!(joined.contains("Disconnected"), "audit: {joined}");

    let _ = std::fs::remove_dir_all(&cfg_dir);
}
