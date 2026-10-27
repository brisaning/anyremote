//! Gate C-2 — **permisos dinámicos**: el anfitrión activa/desactiva entrada y
//! audio en caliente durante la sesión, por stdin (`{"perm":{...}}`), sin
//! reconectar. Cubre: entrada on→off→on, audio off→on (reinicio de captura)
//! y la fuente `mic` en `audio_start`.

use std::path::PathBuf;
use std::time::Duration;

use anyremote_audio::{AudioMode, AudioSource};
use anyremote_daemon::{run_connect, run_listen, ClipboardMode, DaemonConfig};

fn temp_cfg_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("anyremote-dynperm-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("config dir");
    d
}

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind probe");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

fn base_cfg(cfg_dir: PathBuf) -> DaemonConfig {
    let mut c = DaemonConfig::new(cfg_dir);
    c.clipboard = ClipboardMode::Off;
    c.max_secs = 10;
    c
}

fn events_json(events: &[String]) -> Vec<serde_json::Value> {
    events
        .iter()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn count_event(events: &[serde_json::Value], name: &str) -> usize {
    events.iter().filter(|e| e["event"] == name).count()
}

/// Entrada: el anfitrión desactiva y reactiva el control en caliente.
/// Línea temporal (márgenes de ~300 ms en localhost):
/// t=0.25 key1 (inyectada) · t=0.75 `input:false` · t=1.25 key2 (denegada) ·
/// t=1.75 `input:true` · t=2.25 key3 (inyectada) · t=2.75 bye.
#[tokio::test]
async fn input_toggles_at_runtime() {
    let cfg_dir = temp_cfg_dir("entrada");
    let port = free_port().await;

    let (tx_listen, rx_listen) = tokio::sync::mpsc::channel::<String>(8);
    let listen_dir = cfg_dir.clone();
    let listen_events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let ev_sink = listen_events.clone();
    let listen = {
        let mut c = base_cfg(listen_dir);
        c.bind = format!("127.0.0.1:{port}");
        c.allow_input = true; // capacidad (flag); el toggle inicial lo da la UI
        c.bye_source = Some(rx_listen);
        c.on_event = Some(Box::new(move |line| ev_sink.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Lado controlador: secuencia de teclas y cierre con timings.
    let (tx_ctl, rx_ctl) = tokio::sync::mpsc::channel::<String>(8);
    tokio::spawn(async move {
        for (ms, line) in [
            (
                250u64,
                r#"{"input":{"key":{"code":0,"ch":"a","down":true}}}"#,
            ),
            (
                1_250,
                r#"{"input":{"key":{"code":0,"ch":"b","down":true}}}"#,
            ),
            (
                2_250,
                r#"{"input":{"key":{"code":0,"ch":"c","down":true}}}"#,
            ),
            (2_750, "bye"),
        ] {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            let _ = tx_ctl.send(line.to_string()).await;
        }
    });
    let ctl_dir = cfg_dir.clone();
    let controller = {
        let mut c = base_cfg(ctl_dir);
        c.target = Some(format!("127.0.0.1:{port}"));
        c.bye_source = Some(rx_ctl);
        tokio::spawn(async move { run_connect(c).await })
    };

    // El anfitrión apaga a los 0.75 s y enciende a los 1.75 s.
    tokio::time::sleep(Duration::from_millis(750)).await;
    let _ = tx_listen
        .send(r#"{"perm":{"input":false,"audio":false,"mic":false}}"#.to_string())
        .await;
    tokio::time::sleep(Duration::from_millis(1_000)).await;
    let _ = tx_listen
        .send(r#"{"perm":{"input":true,"audio":false,"mic":false}}"#.to_string())
        .await;

    let ctl = controller
        .await
        .expect("join controller")
        .expect("controller ok");
    let srv = listen.await.expect("join listener").expect("listener ok");

    let ev = events_json(&srv.events);
    assert_eq!(
        count_event(&ev, "input_injected"),
        2,
        "key1 y key3 inyectadas: {:?}",
        srv.events
    );
    // Acuse (RF-02a): el controlador recibe confirmación de cada evento —
    // 2 inyectadas (ok) y 1 rechazada (key2 con el toggle apagado).
    let ctl_ev = events_json(&ctl.events);
    let acks: Vec<&serde_json::Value> = ctl_ev
        .iter()
        .filter(|e| e["event"] == "input_result")
        .collect();
    let oks = acks.iter().filter(|e| e["ok"] == true).count();
    let denied = acks.iter().filter(|e| e["ok"] == false).count();
    assert_eq!((acks.len(), oks, denied), (3, 2, 1), "acuses: {acks:?}");
    assert_eq!(
        count_event(&ev, "input_denied"),
        1,
        "key2 denegada con el toggle apagado: {:?}",
        srv.events
    );
    assert_eq!(
        count_event(&ev, "permissions_updated"),
        2,
        "dos comandos aplicados: {:?}",
        srv.events
    );
    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// Audio: el anfitrión apaga y reactiva la captura en caliente (reinicio) y la
/// fuente pedida (`mic`) se refleja en `audio_start`.
#[tokio::test]
async fn audio_toggles_at_runtime_and_reports_source() {
    let cfg_dir = temp_cfg_dir("audio");
    let port = free_port().await;

    let (tx_listen, rx_listen) = tokio::sync::mpsc::channel::<String>(8);
    let listen_dir = cfg_dir.clone();
    let listen_events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let ev_sink = listen_events.clone();
    let listen = {
        let mut c = base_cfg(listen_dir);
        c.bind = format!("127.0.0.1:{port}");
        c.audio = AudioMode::Test;
        c.audio_sources = vec![AudioSource::Microphone]; // fuente pedida (Test la ignora)
        c.bye_source = Some(rx_listen);
        c.on_event = Some(Box::new(move |line| ev_sink.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(150)).await;

    let (tx_ctl, rx_ctl) = tokio::sync::mpsc::channel::<String>(4);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(2_500)).await;
        let _ = tx_ctl.send("bye".to_string()).await;
    });
    let ctl_dir = cfg_dir.clone();
    let controller = {
        let mut c = base_cfg(ctl_dir);
        c.target = Some(format!("127.0.0.1:{port}"));
        c.audio = AudioMode::Test;
        c.bye_source = Some(rx_ctl);
        tokio::spawn(async move { run_connect(c).await })
    };

    // Apaga a los 0.5 s y reactiva a los 1.5 s.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let _ = tx_listen
        .send(r#"{"perm":{"input":false,"audio":false,"mic":false}}"#.to_string())
        .await;
    tokio::time::sleep(Duration::from_millis(1_000)).await;
    let _ = tx_listen
        .send(r#"{"perm":{"input":false,"audio":true,"mic":false}}"#.to_string())
        .await;

    let ctl = controller
        .await
        .expect("join controller")
        .expect("controller ok");
    let srv = listen.await.expect("join listener").expect("listener ok");

    let ev = events_json(&srv.events);
    // Fuente inicial (`mic`) y reinicio tras reactivar.
    assert!(
        ev.iter()
            .any(|e| e["event"] == "audio_start" && e["source"] == "mic"),
        "audio_start debe indicar la fuente mic: {:?}",
        srv.events
    );
    assert_eq!(
        count_event(&ev, "audio_start"),
        2,
        "captura arrancada dos veces: {:?}",
        srv.events
    );
    assert_eq!(
        count_event(&ev, "audio_stopped"),
        1,
        "detenida al apagar: {:?}",
        srv.events
    );
    assert_eq!(
        count_event(&ev, "permissions_updated"),
        2,
        "{:?}",
        srv.events
    );
    // El controlador recibió audio (antes de apagar y tras reactivar).
    assert!(
        ctl.report.audio_recv > 0,
        "frames de audio recibidos: {}",
        ctl.report.audio_recv
    );
    let ctl_ev = events_json(&ctl.events);
    assert!(
        count_event(&ctl_ev, "audio_stats") >= 1,
        "audio_stats en el controlador"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}
