//! C-13: adaptación por red de punta a punta — el CONTROLADOR mide el RTT
//! (aquí forzado con `net_rtt_override`, porque en loopback es ~0 ms) y lo
//! devuelve por `NetHint` al anfitrión, donde el adaptador baja el bitrate y
//! el informe del host registra heartbeats/rtt reales.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyremote_daemon::video::VideoMode;
use anyremote_daemon::{run_connect, run_listen, ClipboardMode, DaemonConfig};

fn mk(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("anyremote-net-{tag}-{}", std::process::id()));
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

#[tokio::test]
async fn controller_rtt_hint_makes_the_host_adapt_bitrate() {
    let cfg_dir = mk("red");
    let port = free_port().await;

    // Anfitrión: vídeo en modo Test (sin hardware) con tope → margen para bajar.
    let listen_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let le = listen_events.clone();
    let listen = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.bind = format!("127.0.0.1:{port}");
        c.clipboard = ClipboardMode::Off;
        c.video = VideoMode::Test;
        c.video_size = (960, 540);
        c.video_fps = 15;
        c.video_max_bps = Some(600_000);
        c.max_secs = 15;
        c.on_event = Some(Box::new(move |line: String| le.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Controlador: RTT «de campo» forzado a 500 ms → congestión sostenida.
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(5500)).await;
        let _ = tx.send("bye".to_string()).await;
    });
    let ctl = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Off;
        c.bye_source = Some(rx);
        c.max_secs = 8;
        c.net_rtt_override = Some(500);
        run_connect(c).await.expect("controller ok")
    };

    let srv = tokio::time::timeout(Duration::from_secs(12), listen)
        .await
        .expect("el listener termina su sesión")
        .expect("join listener")
        .expect("listener ok");

    // El anfitrión adaptó por RED (3 confirmaciones de 1 s → ~3 s de sesión).
    let text = srv.events.join("\n");
    assert!(
        text.contains("\"reason\":\"red_bitrate\"") || text.contains("\"reason\":\"red_escala\""),
        "video_adapt con motivo de RED:\n{text}"
    );
    assert!(
        text.contains("\"event\":\"video_adapt\""),
        "emite video_adapt:\n{text}"
    );
    // El informe del HOST ahora registra los latidos devueltos por NetHint
    // (antes: heartbeats=0 y rtt_min ausente en el anfitrión).
    assert!(
        srv.report.heartbeats_ok >= 3,
        "heartbeats del host desde la pista de red: {}",
        srv.report.heartbeats_ok
    );
    assert!(
        srv.report.rtt_min_ms.is_some(),
        "rtt_min del host presente: {:?}",
        srv.report.rtt_min_ms
    );
    // Y el controlador midió su propio RTT (loopback ≈ 0).
    assert!(ctl.report.heartbeats_ok >= 1);

    let _ = std::fs::remove_dir_all(&cfg_dir);
}
