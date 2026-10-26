//! Regresión del fallo observado en prueba cross-device real (sesión S5663BFII2):
//! el lado remoto murió ~1 s tras recibir un ClipboardSync (error de portapapeles
//! real → `?` fatal). Hoy: un fallo de portapapeles desactiva SOLO ese subsistema
//! (`clipboard_error`) y la sesión continúa hasta su cierre ordenado.

use std::sync::{Arc, Mutex};

use anyremote_daemon::{run_connect, run_listen, ClipboardMode, DaemonConfig};

fn mk(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("anyremote-clip-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    d
}

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("probe");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

#[tokio::test]
async fn clipboard_faults_do_not_kill_the_session() {
    let cfg_dir = mk("flaky");
    let port = free_port().await;

    let listen_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let le = listen_events.clone();
    let listen_cfg = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.bind = format!("127.0.0.1:{port}");
        c.clipboard = ClipboardMode::Flaky; // fallas simuladas en el receptor
        c.max_secs = 4;
        c.on_event = Some(Box::new(move |l: String| le.lock().unwrap().push(l)));
        c
    };
    let listen = tokio::spawn(async move { run_listen(listen_cfg).await });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let ctl_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let ce = ctl_events.clone();
    let connect_cfg = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Flaky; // fallas simuladas también en el emisor
        c.clipboard_seed = Some("primera lectura ok".into());
        c.max_secs = 3;
        c.on_event = Some(Box::new(move |l: String| ce.lock().unwrap().push(l)));
        c
    };
    let controller = tokio::spawn(async move { run_connect(connect_cfg).await });

    // AMBOS lados deben terminar limpiamente (Ok), no con error/crash.
    let ctl = controller.await.expect("join ctl").expect("sesión controlador completa");
    let srv = listen.await.expect("join listener").expect("listener completa (no murió por clipboard)");

    let ctl_text = ctl_events.lock().unwrap().join("\n");
    let srv_text = listen_events.lock().unwrap().join("\n");
    eprintln!("=== CTL ===\n{ctl_text}\n=== SRV ===\n{srv_text}");

    // El controlador: su primera lectura salió (envió), la segunda falló → clipboard_error.
    assert!(
        ctl_text.contains("\"event\":\"clipboard_error\""),
        "falta clipboard_error del controlador:\n{ctl_text}"
    );
    assert!(
        ctl_text.contains("\"event\":\"session_active\""),
        "la sesión debió establecerse:\n{ctl_text}"
    );
    assert!(
        ctl_text.contains("\"event\":\"closed\""),
        "la sesión debió cerrarse ordenadamente:\n{ctl_text}"
    );
    // Latidos sobrevivieron a la caída del portapapeles.
    assert!(
        ctl.report.heartbeats_ok >= 1,
        "los latidos deben seguir tras el fallo de clipboard: {}",
        ctl.report.heartbeats_ok
    );

    // El receptor (escrituras de portapapeles que fallan al aplicar) también sobrevive.
    assert!(
        srv_text.contains("\"event\":\"session_active\""),
        "receptor: sesión activa\n{srv_text}"
    );
    assert!(
        srv_text.contains("\"event\":\"clipboard_error\"") || srv_text.contains("\"event\":\"closed\""),
        "receptor: debe registrar el fallo de aplicación o cerrar ordenado:\n{srv_text}"
    );
    assert!(
        srv_text.contains("\"event\":\"closed\""),
        "receptor: cierre ordenado (no muerte súbita):\n{srv_text}"
    );
    let _ = srv;
    let _ = std::fs::remove_dir_all(&cfg_dir);
}
