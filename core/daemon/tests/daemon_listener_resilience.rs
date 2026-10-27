//! Regresión del fallo observado en prueba cross-device: un cliente que aborta
//! (cierre súbito o basura antes del TLS) NO debe matar al listener — este
//! debe seguir aceptando y completar la sesión con el siguiente cliente.

use std::sync::{Arc, Mutex};

use anyremote_daemon::{run_connect, run_listen, ClipboardMode, DaemonConfig};

type PathBufFix = std::path::PathBuf;

fn mk(tag: &str) -> PathBufFix {
    let d = std::env::temp_dir().join(format!("anyremote-resil-{tag}-{}", std::process::id()));
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
async fn listener_survives_aborted_clients_and_serves_the_next_one() {
    let cfg_dir = mk("survive");
    let port = free_port().await;

    let listener_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let le = listener_events.clone();
    let listen_cfg = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.bind = format!("127.0.0.1:{port}");
        c.clipboard = ClipboardMode::Memory;
        c.max_secs = 15;
        c.on_event = Some(Box::new(move |line: String| {
            le.lock().unwrap().push(line);
        }));
        c
    };
    let listen = tokio::spawn(async move { run_listen(listen_cfg).await });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // 1) Cliente que conecta y ABORTA de inmediato (caso del usuario: pin/validación
    //    falló en el cliente y cerró el socket antes del TLS).
    let bad1 = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect bad1");
    drop(bad1);

    // 2) Cliente que manda basura (rompe bootstrap/TLS).
    use tokio::io::AsyncWriteExt;
    let mut bad2 = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect bad2");
    bad2.write_all(b"GARBAGE-NO-ES-CERT\n")
        .await
        .expect("write");
    drop(bad2);

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // El listener DEBE seguir vivo: un cliente bueno completa la sesión.
    let ctl_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let ce = ctl_events.clone();
    let connect_cfg = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Memory;
        c.max_secs = 3;
        c.on_event = Some(Box::new(move |line: String| {
            ce.lock().unwrap().push(line);
        }));
        c
    };
    let controller = tokio::spawn(async move { run_connect(connect_cfg).await });

    let ctl = controller
        .await
        .expect("join ctl")
        .expect("cliente bueno OK");
    let ev_text = ctl_events.lock().unwrap().join("\n");
    assert!(
        ev_text.contains("\"event\":\"session_active\""),
        "el listener debió seguir vivo para servir al2º cliente:\n{ev_text}"
    );
    assert!(ctl.report.heartbeats_ok >= 1);

    // El listener registra los fallos de los clientes malos y termina con la sesión buena.
    let srv = listen.await.expect("join listener").expect("listener OK");
    let srv_text = srv.events.join("\n");
    let client_failures = srv_text.matches("\"event\":\"client_failed\"").count();
    assert!(
        client_failures >= 1,
        "los clientes fallidos deben auditar client_failed:\n{srv_text}"
    );
    assert!(
        srv_text.contains("\"event\":\"session_active\""),
        "la sesión buena debe registrarse"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}
