//! Gate Hito M2 — streaming de vídeo: listen con `--video test` envía `VideoFrame`
//! por la sesión; el controlador decodifica, emite `video_stats` y sirve el preview
//! local (verificado con un cliente TCP real que valida la cabecera BGRA).

use std::sync::{Arc, Mutex};

use anyremote_codec::CodecFormat;
use anyremote_daemon::{run_connect, run_listen, video::VideoMode, ClipboardMode, DaemonConfig};

fn mk(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("anyremote-m2-{tag}-{}", std::process::id()));
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

#[tokio::test]
async fn m2_video_streams_and_preview_delivers_frames() {
    let cfg_dir = mk("video");
    let port = free_port().await;

    let listen_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let le = listen_events.clone();
    let listen_cfg = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.bind = format!("127.0.0.1:{port}");
        c.clipboard = ClipboardMode::Off;
        c.video = VideoMode::Test;
        c.video_size = (320, 200);
        c.video_fps = 10;
        c.codec = CodecFormat::Deflate; // rápido y determinista para el gate
        c.max_secs = 5;
        c.on_event = Some(Box::new(move |l: String| le.lock().unwrap().push(l)));
        c
    };
    let listen = tokio::spawn(async move { run_listen(listen_cfg).await });

    let ctl_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let ce = ctl_events.clone();

    // Lector de preview: espera al evento `preview` del controlador y valida la cabecera.
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

    let connect_cfg = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Off;
        c.max_secs = 4;
        c.on_event = Some(Box::new(move |l: String| ce.lock().unwrap().push(l)));
        c
    };
    let controller = tokio::spawn(async move { run_connect(connect_cfg).await });

    let ctl = controller
        .await
        .expect("join ctl")
        .expect("sesión completa");
    let srv = listen
        .await
        .expect("join listener")
        .expect("listener completa");
    let header = preview_reader
        .await
        .expect("join preview")
        .expect("preview entregó cabecera");

    // --- Vídeo fluyó por la sesión y se decodificó ---
    let ctl_text = ctl_events.lock().unwrap().join("\n");
    let srv_text = listen_events.lock().unwrap().join("\n");
    assert!(
        srv_text.contains("\"event\":\"video_start\""),
        "listener inicia vídeo:\n{srv_text}"
    );
    assert!(
        ctl_text.contains("\"event\":\"video_stats\""),
        "controlador decodifica y reporta:\n{ctl_text}"
    );
    assert!(ctl.report.bytes_recv > 0, "bytes de vídeo recibidos");
    assert!(srv.report.bytes_sent > 0, "bytes de vídeo enviados");

    // --- Preview local: cabecera BGRA correcta ---
    let (w, h, _seq, len) = header;
    assert_eq!((w, h), (320, 200), "dimensiones del preview");
    assert_eq!(len, 320 * 200 * 4, "payload BGRA completo");

    let _ = std::fs::remove_dir_all(&cfg_dir);
}
