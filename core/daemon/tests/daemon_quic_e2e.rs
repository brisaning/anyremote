//! Gate C-15 — transporte UDP/QUIC de punta a punta (specs/02 §8):
//!
//! 1. `quic_session_carries_video_and_control` — ambos extremos con
//!    `Transport::Udp`: la sesión completa (auth C2 con binding exportado por
//!    QUIC + hello/permisos + vídeo de prueba) fluye con el vídeo por el
//!    **stream dedicado** (dos bi-streams con prologo) — sin `quic_fallback`.
//! 2. `udp_client_falls_back_to_tcp_when_peer_is_tcp_only` — el cliente UDP
//!    contra un listener solo-TCP emite `quic_fallback` y la sesión continúa
//!    por TCP (compatibilidad con versiones anteriores).
//! 3. `tcp_client_connects_to_dual_listener` — un cliente TCP (peers viejos)
//!    entra al listener `udp` por su socket TCP (listen-udp ata ambos).

use std::sync::{Arc, Mutex};

use anyremote_codec::CodecFormat;
use anyremote_daemon::{
    run_connect, run_listen, video::VideoMode, ClipboardMode, DaemonConfig, Transport,
};

fn mk(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("anyremote-quic-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    d
}

async fn free_port() -> u16 {
    // Rango NO efímero (los efímeros los recicla el SO agresivamente: bajo
    // carga de la suite completa otro test podía tomar el puerto entre el
    // probe y el bind del listener → «connection refused» intermitente).
    // Base por PID (distinta entre binarios) + offset atómico (los 3 tests
    // de este binario usan rangos solapados imposibles).
    use std::sync::atomic::{AtomicU16, Ordering};
    static NEXT: AtomicU16 = AtomicU16::new(0);
    let off = NEXT.fetch_add(4, Ordering::SeqCst);
    let base = 41_000u16
        .wrapping_add((std::process::id() as u16).wrapping_mul(7) % 4_000)
        .wrapping_add(off);
    for port in [base, base + 1, base + 2, base + 3] {
        let tcp = tokio::net::TcpListener::bind(("127.0.0.1", port)).await;
        let udp = tokio::net::UdpSocket::bind(("127.0.0.1", port)).await;
        if let (Ok(_), Ok(_)) = (tcp, udp) {
            return port; // libre en TCP y UDP (el listener udp ata ambos)
        }
    }
    panic!("sin puerto libre en {base}..{}", base + 3);
}

fn find_preview_port(events: &Arc<Mutex<Vec<String>>>) -> Option<u16> {
    for line in events.lock().unwrap().iter() {
        if !line.contains("\"event\":\"preview\"") {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(p) = v.get("port").and_then(|p| p.as_u64()) {
                return Some(p as u16);
            }
        }
    }
    None
}

/// Config del listener: vídeo de prueba determinista (deflate, patrón M2).
fn listen_cfg(
    dir: std::path::PathBuf,
    port: u16,
    transport: Transport,
    events: Arc<Mutex<Vec<String>>>,
) -> DaemonConfig {
    let mut c = DaemonConfig::new(dir);
    c.bind = format!("127.0.0.1:{port}");
    c.clipboard = ClipboardMode::Off;
    c.transport = transport;
    c.video = VideoMode::Test;
    c.video_size = (320, 200);
    c.video_fps = 10;
    c.codec = CodecFormat::Deflate;
    c.max_secs = 8;
    c.on_event = Some(Box::new(move |l: String| events.lock().unwrap().push(l)));
    c
}

fn connect_cfg(
    dir: std::path::PathBuf,
    port: u16,
    transport: Transport,
    events: Arc<Mutex<Vec<String>>>,
) -> DaemonConfig {
    let mut c = DaemonConfig::new(dir);
    c.target = Some(format!("127.0.0.1:{port}"));
    c.clipboard = ClipboardMode::Off;
    c.transport = transport;
    c.max_secs = 4;
    c.on_event = Some(Box::new(move |l: String| events.lock().unwrap().push(l)));
    c
}

/// Escenario 1: sesión QUIC completa con vídeo por el stream dedicado.
#[tokio::test]
async fn quic_session_carries_video_and_control() {
    let cfg_dir = mk("session");
    let port = free_port().await;

    let listen_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let listen = tokio::spawn({
        let cfg_dir = cfg_dir.clone();
        let le = listen_events.clone();
        async move { run_listen(listen_cfg(cfg_dir, port, Transport::Udp, le)).await }
    });

    let ctl_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let ce = ctl_events.clone();
    let preview_reader = {
        let ce = ce.clone();
        tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(6);
            loop {
                if tokio::time::Instant::now() > deadline {
                    return None;
                }
                if let Some(p) = find_preview_port(&ce) {
                    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", p))
                        .await
                        .expect("conectar al preview local");
                    let mut s = tcp;
                    use tokio::io::AsyncReadExt;
                    let mut hdr = [0u8; 16];
                    if s.read_exact(&mut hdr).await.is_err() {
                        return None;
                    }
                    let w = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
                    let h = u32::from_le_bytes(hdr[4..8].try_into().unwrap());
                    let seq = u32::from_le_bytes(hdr[8..12].try_into().unwrap());
                    let len = u32::from_le_bytes(hdr[12..16].try_into().unwrap());
                    return Some((w, h, seq, len));
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        })
    };

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let controller = tokio::spawn({
        let cfg_dir = cfg_dir.clone();
        let ce = ctl_events.clone();
        async move { run_connect(connect_cfg(cfg_dir, port, Transport::Udp, ce)).await }
    });

    let ctl = controller
        .await
        .expect("join ctl")
        .expect("sesión QUIC completa");
    let srv = listen
        .await
        .expect("join listener")
        .expect("listener completo");
    let header = preview_reader
        .await
        .expect("join preview")
        .expect("preview entregó cabecera");

    let ctl_text = ctl_events.lock().unwrap().join("\n");
    let srv_text = listen_events.lock().unwrap().join("\n");

    // Ambos lados declararon el transporte UDP (evento `listening`/`connecting`).
    assert!(
        srv_text.contains("\"transport\":\"udp\""),
        "listener anuncia udp:\n{srv_text}"
    );
    assert!(
        ctl_text.contains("\"transport\":\"udp\""),
        "controlador anuncia udp:\n{ctl_text}"
    );
    // NO hubo fallback: la sesión fue QUIC de verdad.
    assert!(
        !ctl_text.contains("quic_fallback"),
        "sin fallback QUIC→TCP:\n{ctl_text}"
    );
    // Auth C2 completada con binding QUIC exportado (evento de UI del
    // anfitrión; el detalle de permisos vive en la auditoría interna).
    assert!(
        srv_text.contains("\"event\":\"session_active\""),
        "auth C2 sobre QUIC:\n{srv_text}"
    );
    assert!(
        ctl_text.contains("\"event\":\"session_active\""),
        "controlador autenticado:\n{ctl_text}"
    );
    // Vídeo (por el stream dedicado) fluyó y se decodificó en el visor.
    assert!(
        srv_text.contains("\"event\":\"video_start\""),
        "listener inicia vídeo:\n{srv_text}"
    );
    assert!(
        ctl_text.contains("\"event\":\"video_stats\""),
        "controlador decodifica vídeo (stream dedicado QUIC):\n{ctl_text}"
    );
    assert!(ctl.report.bytes_recv > 0, "bytes de vídeo recibidos");
    assert!(srv.report.bytes_sent > 0, "bytes de vídeo enviados");
    // Control (heartbeats + NetHint C-13) viajó por el stream de control.
    assert!(ctl.report.heartbeats_ok > 0, "heartbeats en QUIC");

    // Preview local: cabecera BGRA correcta (frames llegaron completos).
    let (w, h, _seq, len) = header;
    assert_eq!((w, h), (320, 200), "dimensiones del preview");
    assert_eq!(len, 320 * 200 * 4, "payload BGRA completo");

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// Escenario 2: cliente UDP contra listener SOLO-TCP → fallback auditable y
/// sesión por TCP (compat con versiones anteriores).
#[tokio::test]
async fn udp_client_falls_back_to_tcp_when_peer_is_tcp_only() {
    let cfg_dir = mk("fallback");
    let port = free_port().await;

    let listen_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let listen = tokio::spawn({
        let cfg_dir = cfg_dir.clone();
        let le = listen_events.clone();
        async move { run_listen(listen_cfg(cfg_dir, port, Transport::Tcp, le)).await }
    });

    let ctl_events = Arc::new(Mutex::new(Vec::<String>::new()));
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let controller = {
        let cfg_dir = cfg_dir.clone();
        let ce = ctl_events.clone();
        tokio::spawn(
            async move { run_connect(connect_cfg(cfg_dir, port, Transport::Udp, ce)).await },
        )
    };

    let ctl = controller
        .await
        .expect("join ctl")
        .expect("sesión (por TCP) completa");
    let _srv = listen
        .await
        .expect("join listener")
        .expect("listener completo");

    let ctl_text = ctl_events.lock().unwrap().join("\n");
    assert!(
        ctl_text.contains("\"event\":\"quic_fallback\""),
        "evento quic_fallback emitido:\n{ctl_text}"
    );
    assert!(
        ctl_text.contains("\"event\":\"video_stats\""),
        "vídeo fluyó tras el fallback:\n{ctl_text}"
    );
    assert!(ctl.report.bytes_recv > 0, "bytes de vídeo por TCP");

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// Escenario 3: cliente TCP (peer viejo) contra listener `udp` → entra por el
/// socket TCP (listen-udp ata ambos, compatibilidad de ida y vuelta).
#[tokio::test]
async fn tcp_client_connects_to_dual_listener() {
    let cfg_dir = mk("compat");
    let port = free_port().await;

    let listen_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let listen = tokio::spawn({
        let cfg_dir = cfg_dir.clone();
        let le = listen_events.clone();
        async move { run_listen(listen_cfg(cfg_dir, port, Transport::Udp, le)).await }
    });

    let ctl_events = Arc::new(Mutex::new(Vec::<String>::new()));
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let controller = {
        let cfg_dir = cfg_dir.clone();
        let ce = ctl_events.clone();
        tokio::spawn(
            async move { run_connect(connect_cfg(cfg_dir, port, Transport::Tcp, ce)).await },
        )
    };

    let ctl = controller
        .await
        .expect("join ctl")
        .expect("sesión TCP completa");
    let _srv = listen
        .await
        .expect("join listener")
        .expect("listener completo");

    let ctl_text = ctl_events.lock().unwrap().join("\n");
    let srv_text = listen_events.lock().unwrap().join("\n");
    // El cliente TCP no intenta QUIC → sin fallback; sesión normal.
    assert!(
        !ctl_text.contains("quic_fallback"),
        "cliente TCP no genera fallback:\n{ctl_text}"
    );
    assert!(
        ctl_text.contains("\"event\":\"video_stats\""),
        "vídeo fluyó por TCP contra el listener dual:\n{ctl_text}"
    );
    assert!(
        srv_text.contains("\"event\":\"session_active\""),
        "sesión aceptada por el socket TCP del listener dual:\n{srv_text}"
    );
    assert!(ctl.report.bytes_recv > 0, "bytes de vídeo recibidos");

    let _ = std::fs::remove_dir_all(&cfg_dir);
}
