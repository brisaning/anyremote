//! C-11: overlay del cursor remoto — el anfitrión sondea el cursor y envía
//! **solo cambios** (canal `watch`); el controlador los emite como eventos
//! `cursor` para el overlay del visor (sin inundar la auditoría).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyremote_daemon::{run_connect, run_listen, ClipboardMode, DaemonConfig};
use anyremote_input::cursor::{CursorBackend, CursorPos};

fn mk(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("anyremote-cursor-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    d
}

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("probe");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

/// Sesión completa con el backend de cursor indicado en el ANFITRIÓN.
/// Devuelve los eventos JSONL del CONTROLADOR (dónde nace el overlay).
async fn session_with_cursor(
    cfg_dir: &std::path::Path,
    backend: CursorBackend,
) -> Vec<serde_json::Value> {
    let port = free_port().await;

    let listen = {
        let mut c = DaemonConfig::new(cfg_dir.to_path_buf());
        c.bind = format!("127.0.0.1:{port}");
        c.clipboard = ClipboardMode::Off;
        c.max_secs = 15;
        c.cursor_backend = backend;
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;

    let ctl_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let ce = ctl_events.clone();
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(700)).await;
        let _ = tx.send("bye".to_string()).await;
    });
    let ctl = {
        let mut c = DaemonConfig::new(cfg_dir.to_path_buf());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Off;
        c.bye_source = Some(rx);
        c.max_secs = 5;
        c.on_event = Some(Box::new(move |line: String| ce.lock().unwrap().push(line)));
        run_connect(c).await.expect("controller ok")
    };
    let _srv = listen.await.expect("join listener").expect("listener ok");
    let _ = ctl;

    let out: Vec<serde_json::Value> = ctl_events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    out
}

fn cursor_events(events: &[serde_json::Value]) -> Vec<&serde_json::Value> {
    events.iter().filter(|e| e["event"] == "cursor").collect()
}

#[tokio::test]
async fn fixed_cursor_sends_exactly_one_event() {
    let cfg_dir = mk("fijo");
    let pos = CursorPos {
        nx: 1000,
        ny: 2000,
        visible: true,
    };
    let events = session_with_cursor(&cfg_dir, CursorBackend::Fixed(pos)).await;

    let cursors = cursor_events(&events);
    assert_eq!(
        cursors.len(),
        1,
        "posición FIJA → exactamente UN evento (change-detection):\n{cursors:?}"
    );
    assert_eq!(cursors[0]["nx"], 1000);
    assert_eq!(cursors[0]["ny"], 2000);
    assert_eq!(cursors[0]["visible"], true);

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

#[tokio::test]
async fn moving_cursor_streams_updates_in_range() {
    let cfg_dir = mk("movil");
    let p0 = CursorPos {
        nx: 0,
        ny: 0,
        visible: false,
    };
    let p1 = CursorPos {
        nx: 65535,
        ny: 32768,
        visible: true,
    };
    let events = session_with_cursor(
        &cfg_dir,
        CursorBackend::Sequence {
            points: vec![p0, p1],
            idx: 0,
        },
    )
    .await;

    let cursors = cursor_events(&events);
    assert!(
        cursors.len() >= 2,
        "cursor en movimiento → varios eventos:\n{cursors:?}"
    );
    // Primer estado = primera posición del guion.
    assert_eq!(cursors[0]["nx"], 0);
    assert_eq!(cursors[0]["visible"], false);
    for e in &cursors {
        assert!(e["nx"].as_u64().unwrap_or(999999) <= 65535, "nx en rango");
        assert!(e["ny"].as_u64().unwrap_or(999999) <= 65535, "ny en rango");
    }
    // Al menos un cambio hacia la segunda posición (visibilidad incluida).
    assert!(
        cursors
            .iter()
            .any(|e| e["nx"] == 65535 && e["visible"] == true),
        "el segundo estado llega al visor:\n{cursors:?}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}
