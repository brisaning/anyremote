//! Gate D-03 — **identidad persistente + trust por dispositivo**: con el mismo
//! directorio de configuración (mismo equipo), el `NodeId` es estable entre
//! sesiones (`session_id` idéntico) y el par autenticado pasa a
//! `peer_trusted=true` en la segunda sesión (TOFU).

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyremote_daemon::{run_connect, run_listen, ClipboardMode, DaemonConfig, RunOutcome};
use anyremote_telemetry::AuditEventKind;

fn temp_cfg_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("anyremote-ident-{tag}-{}", std::process::id()));
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

/// Sesión completa (listen + connect) sobre el MISMO directorio de config.
async fn session(cfg_dir: &PathBuf) -> (RunOutcome, RunOutcome) {
    let port = free_port().await;
    let listen_dir = cfg_dir.clone();
    let listen = tokio::spawn(async move {
        let mut c = DaemonConfig::new(listen_dir);
        c.bind = format!("127.0.0.1:{port}");
        c.clipboard = ClipboardMode::Off;
        c.max_secs = 6;
        run_listen(c).await
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let _ = tx.send("bye".to_string()).await;
    });
    let ctl_dir = cfg_dir.clone();
    let ctl = {
        let mut c = DaemonConfig::new(ctl_dir);
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Off;
        c.bye_source = Some(rx);
        c.max_secs = 5;
        run_connect(c).await.expect("controller ok")
    };
    let srv = listen.await.expect("join listener").expect("listener ok");
    (ctl, srv)
}

/// `peer_trusted` del `session_established` de cada sesión de un rol, en orden
/// cronológico (los archivos se separan por rol: solo el controlador tiene
/// `session_started`).
fn trusted_flags(cfg_dir: &Path, controller_side: bool) -> Vec<bool> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(cfg_dir.join("audit"))
        .expect("audit dir")
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH)
    });
    let mut out = Vec::new();
    for f in files {
        let evs = anyremote_telemetry::SessionLog::read_all(&f).expect("jsonl válido");
        let started = evs
            .iter()
            .any(|e| e.kind == AuditEventKind::SessionStarted);
        if started != controller_side {
            continue;
        }
        if let Some(e) = evs
            .iter()
            .find(|e| e.kind == AuditEventKind::SessionEstablished)
        {
            out.push(
                e.detail
                    .get("peer_trusted")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            );
        }
    }
    out
}

#[tokio::test]
async fn identity_is_stable_and_peer_becomes_trusted() {
    let cfg_dir = temp_cfg_dir("identidad");

    // Sesión 1: store vacío → par desconocido (peer_trusted=false).
    let (ctl1, _srv1) = session(&cfg_dir).await;
    // Sesión 2: mismo equipo (mismo config dir) → identidad recordada.
    let (ctl2, _srv2) = session(&cfg_dir).await;

    // Identidad estable (D-03): el session_id deriva del NodeId → idéntico.
    assert_eq!(
        ctl1.report.session_id, ctl2.report.session_id,
        "el NodeId del equipo debe ser estable entre arranques"
    );

    // TOFU: la segunda sesión del mismo par queda como confiable.
    let ctl_flags = trusted_flags(&cfg_dir, true);
    assert_eq!(ctl_flags, vec![false, true], "controlador: {ctl_flags:?}");

    // El anfitrión también recuerda al controlador.
    let listen_flags = trusted_flags(&cfg_dir, false);
    assert_eq!(listen_flags, vec![false, true], "anfitrión: {listen_flags:?}");

    // El archivo de identidad existe (persistencia real).
    assert!(cfg_dir.join("identity.key").exists(), "identity.key");
    assert!(cfg_dir.join("trusted.ids").exists(), "trusted.ids");

    let _ = std::fs::remove_dir_all(&cfg_dir);
}
