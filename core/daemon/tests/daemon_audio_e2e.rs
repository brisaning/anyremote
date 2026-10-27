//! Gate RF audio: captura `Test` (tono sintético, sin hardware) → Opus →
//! sesión ARSP → decodificación en el controlador con sink `Test` (sin
//! altavoz), bajo permiso `audio` (D-07). Cubre: flujo completo con cierre
//! ordenado, denegación cuando el controlador NO ofrece `audio` y denegación
//! cuando el anfitrión NO comparte.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyremote_audio::AudioMode;
use anyremote_daemon::{run_connect, run_listen, ClipboardMode, DaemonConfig, RunOutcome};

fn temp_cfg_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("anyremote-audio-{tag}-{}", std::process::id()));
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
    c.max_secs = 6;
    c
}

/// Lado controlado: `listen` con el modo de audio indicado.
fn listen_cfg(cfg_dir: PathBuf, port: u16, audio: AudioMode) -> DaemonConfig {
    let mut c = base_cfg(cfg_dir);
    c.bind = format!("127.0.0.1:{port}");
    c.audio = audio;
    c
}

/// Lado controlador: `connect` con `bye` programado tras `bye_after_ms`.
async fn controller(
    cfg_dir: PathBuf,
    port: u16,
    audio: AudioMode,
    bye_after_ms: u64,
) -> RunOutcome {
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(bye_after_ms)).await;
        let _ = tx.send("bye".to_string()).await;
    });
    let mut c = base_cfg(cfg_dir);
    c.target = Some(format!("127.0.0.1:{port}"));
    c.audio = audio;
    c.bye_source = Some(rx);
    run_connect(c).await.expect("controller ok")
}

fn events_json(events: &[String]) -> Vec<serde_json::Value> {
    events
        .iter()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn has_event(events: &[serde_json::Value], name: &str) -> bool {
    events.iter().any(|e| e["event"] == name)
}

fn audit_kinds(cfg_dir: &Path) -> String {
    let mut files = std::fs::read_dir(cfg_dir.join("audit"))
        .expect("audit dir")
        .flatten()
        .map(|e| e.path())
        .collect::<Vec<_>>();
    files.sort();
    let mut kinds = Vec::new();
    for f in &files {
        for ev in anyremote_telemetry::SessionLog::read_all(f).expect("jsonl válido") {
            kinds.push(format!("{:?}", ev.kind));
        }
        for ev in anyremote_telemetry::SessionLog::read_all(f).expect("jsonl válido") {
            kinds.push(ev.detail.to_string());
        }
    }
    kinds.join(",")
}

/// Flujo completo: el anfitrión comparte (`Test`), el controlador ofrece
/// `audio` → se concede, el PCM viaja codificado y el controlador decodifica.
#[tokio::test]
async fn audio_flows_end_to_end_in_test_mode() {
    let cfg_dir = temp_cfg_dir("flujo");
    let port = free_port().await;

    let listen_dir = cfg_dir.clone();
    let listen =
        tokio::spawn(
            async move { run_listen(listen_cfg(listen_dir, port, AudioMode::Test)).await },
        );
    tokio::time::sleep(Duration::from_millis(150)).await;

    let ctl = controller(cfg_dir.clone(), port, AudioMode::Test, 900).await;
    let srv = listen.await.expect("join listener").expect("listener ok");

    // Anfitrión: audio concedido y captura iniciada (48 kHz).
    let srv_ev = events_json(&srv.events);
    assert!(
        has_event(&srv_ev, "audio_start"),
        "listener sin audio_start: {:?}",
        srv.events
    );
    assert!(
        srv.events.iter().any(|l| l.contains("\"rate\":48000")),
        "audio_start debe indicar 48 kHz: {:?}",
        srv.events
    );
    // Controlador: reproductor iniciado y frames decodificados.
    let ctl_ev = events_json(&ctl.events);
    assert!(
        has_event(&ctl_ev, "audio_start"),
        "connect sin audio_start: {:?}",
        ctl.events
    );
    assert!(
        has_event(&ctl_ev, "audio_stats"),
        "sin audio_stats: {:?}",
        ctl.events
    );
    assert!(
        srv.report.audio_sent >= 10 && ctl.report.audio_recv >= 10,
        "audio en el informe: sent={} recv={}",
        srv.report.audio_sent,
        ctl.report.audio_recv
    );
    assert!(
        srv.report.bytes_sent > 0 && ctl.report.bytes_recv > 0,
        "los bytes de audio deben contabilizarse"
    );

    // Concesión de `audio` auditable + cierre ordenado por el bye.
    let audit = audit_kinds(&cfg_dir);
    assert!(audit.contains("PermissionGranted"), "audit: {audit}");
    assert!(
        audit.contains("\"audio\""),
        "el permiso audio debe constar: {audit}"
    );
    let closed = |evs: &[serde_json::Value]| {
        evs.iter()
            .any(|e| e["event"] == "closed" && e["reason"] == "bye")
    };
    assert!(
        closed(&srv_ev),
        "listener sin closed(bye): {:?}",
        srv.events
    );
    assert!(
        closed(&ctl_ev),
        "controller sin closed(bye): {:?}",
        ctl.events
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// El controlador NO ofrece `audio` → el anfitrión no captura ni envía nada.
#[tokio::test]
async fn audio_denied_when_controller_does_not_offer() {
    let cfg_dir = temp_cfg_dir("sin-oferta");
    let port = free_port().await;

    let listen_dir = cfg_dir.clone();
    let listen =
        tokio::spawn(
            async move { run_listen(listen_cfg(listen_dir, port, AudioMode::Test)).await },
        );
    tokio::time::sleep(Duration::from_millis(150)).await;

    let ctl = controller(cfg_dir.clone(), port, AudioMode::Off, 600).await;
    let srv = listen.await.expect("join listener").expect("listener ok");

    let srv_ev = events_json(&srv.events);
    assert!(
        has_event(&srv_ev, "audio_denied"),
        "debe denegarse audio: {:?}",
        srv.events
    );
    assert!(
        !has_event(&srv_ev, "audio_start"),
        "no debe iniciarse captura: {:?}",
        srv.events
    );
    assert_eq!(srv.report.audio_sent, 0, "nada enviado");
    assert_eq!(ctl.report.audio_recv, 0, "nada recibido");
    assert!(
        !has_event(&events_json(&ctl.events), "audio_stats"),
        "sin audio_stats"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// El controlador pide audio pero el anfitrión no comparte → se deniega en el
/// controlador y no se abre ningún dispositivo de reproducción.
#[tokio::test]
async fn audio_denied_when_host_does_not_share() {
    let cfg_dir = temp_cfg_dir("sin-compartir");
    let port = free_port().await;

    let listen_dir = cfg_dir.clone();
    let listen =
        tokio::spawn(async move { run_listen(listen_cfg(listen_dir, port, AudioMode::Off)).await });
    tokio::time::sleep(Duration::from_millis(150)).await;

    let ctl = controller(cfg_dir.clone(), port, AudioMode::Test, 600).await;
    let srv = listen.await.expect("join listener").expect("listener ok");

    let ctl_ev = events_json(&ctl.events);
    assert!(
        has_event(&ctl_ev, "audio_denied"),
        "connect debe denegar: {:?}",
        ctl.events
    );
    // C-2: el reproductor queda preparado (se emite `audio_start`), pero sin
    // frames del anfitrión no hay reproducción ni métricas.
    assert!(
        !has_event(&ctl_ev, "audio_stats"),
        "sin frames → sin audio_stats: {:?}",
        ctl.events
    );
    assert_eq!(ctl.report.audio_recv, 0);
    assert_eq!(srv.report.audio_sent, 0);
    assert!(
        !has_event(&events_json(&srv.events), "audio_start"),
        "el anfitrión ni siquiera debe pedirlo"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}
