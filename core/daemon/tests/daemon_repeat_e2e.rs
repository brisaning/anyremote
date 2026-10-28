//! Listener repetidor (v0.8.2) — regresión del reporte de campo: al cerrar la
//! sesión desde el INVITADO, el anfitrión quedaba «Escuchando (red)» sin poder
//! aceptar reconexiones hasta detener y reactivar la escucha.
//!
//! Cubre tres cosas:
//! 1. `--max-sessions 2`: tras el `bye` de red de la 1ª sesión vuelve a
//!    `accept()` y sirve a un 2º cliente (reconexión sin reiniciar).
//! 2. `bye` por stdin SIN sesión: detiene la escucha (no queda colgado).
//! 3. Proceso real con stdin ABIERTO: al agotar `--max-secs` termina solo
//!    (antes se colgaba en el runtime de tokio esperando a leer stdin).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyremote_daemon::{run_connect, run_listen, ClipboardMode, DaemonConfig, RunOutcome};

fn mk(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("anyremote-repeat-{tag}-{}", std::process::id()));
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

/// Una sesión completa desde el controlador: conecta y la cierra con `bye` (así
/// cierra el invitado al pulsar «Detener»).
async fn session_from_controller(cfg_dir: &std::path::Path, port: u16) -> RunOutcome {
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let _ = tx.send("bye".to_string()).await;
    });
    let mut c = DaemonConfig::new(cfg_dir.to_path_buf());
    c.target = Some(format!("127.0.0.1:{port}"));
    c.clipboard = ClipboardMode::Off;
    c.bye_source = Some(rx);
    c.max_secs = 5;
    run_connect(c).await.expect("controller ok")
}

#[tokio::test]
async fn listener_serves_a_second_session_after_the_invited_closed_the_first() {
    let cfg_dir = mk("dos");
    let port = free_port().await;

    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let ev = events.clone();
    let listen = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.bind = format!("127.0.0.1:{port}");
        c.clipboard = ClipboardMode::Off;
        c.max_secs = 20;
        c.max_sessions = 2; // v0.8.2: dos sesiones y termina
        c.on_event = Some(Box::new(move |line: String| ev.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Sesión 1: el invitado cierra por la RED (bye de red, NO stdin).
    let _ctl1 = session_from_controller(&cfg_dir, port).await;
    // Reconexión inmediata: ése es exactamente el caso de campo que fallaba.
    let _ctl2 = session_from_controller(&cfg_dir, port).await;

    let srv = tokio::time::timeout(Duration::from_secs(8), listen)
        .await
        .expect("el listener termina al completar max_sessions")
        .expect("join listener")
        .expect("listener ok");
    let text = srv.events.join("\n");
    let actives = text.matches("\"event\":\"session_active\"").count();
    assert_eq!(
        actives, 2,
        "dos sesiones servidas sin reiniciar la escucha:\n{text}"
    );
    let closed = text.matches("\"event\":\"closed\"").count();
    assert!(closed >= 2, "cierre auditable por sesión:\n{text}");

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

#[tokio::test]
async fn stdin_bye_stops_the_listener_while_waiting_for_a_client() {
    let cfg_dir = mk("bye");
    let port = free_port().await;

    let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let ev = events.clone();
    let listen = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.bind = format!("127.0.0.1:{port}");
        c.clipboard = ClipboardMode::Off;
        c.max_secs = 30;
        c.max_sessions = 0; // ilimitado: solo lo detiene bye/señal
        c.bye_source = Some(rx);
        c.on_event = Some(Box::new(move |line: String| ev.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Sin sesión: `bye` por stdin debe detener la escucha (antes ni se leía).
    tx.send("bye".to_string()).await.expect("send bye");
    let srv = tokio::time::timeout(Duration::from_secs(3), listen)
        .await
        .expect("el listener responde al bye sin sesión")
        .expect("join listener")
        .expect("listener ok");
    let text = srv.events.join("\n");
    assert!(
        text.contains("\"event\":\"closed\""),
        "cierre audible al detener:\n{text}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// Configuración REAL de la app (`--max-sessions 0`) a nivel de proceso:
/// arranca, recibe el `bye` de «Detener listen» por stdin y sale con código 0
/// (además: el resumen de cierre ya no imprime `Nonems`).
#[test]
fn app_flag_max_sessions_zero_stops_on_stdin_bye() {
    let cfg_dir = std::env::temp_dir().join(format!("anyremote-appflag-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cfg_dir);
    let bin = env!("CARGO_BIN_EXE_anyremoted");
    let mut child = std::process::Command::new(bin)
        .args([
            "listen",
            "--bind",
            "127.0.0.1:0",
            "--max-sessions",
            "0",
            "--video",
            "off",
            "--audio",
            "off",
            "--clipboard",
            "off",
            "--config-dir",
            cfg_dir.to_str().expect("utf8"),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn anyremoted");
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stderr = child.stderr.take().expect("stderr");

    // Espera el evento `listening` leyendo stdout en un hilo (no llenar el pipe).
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
        {
            if line.contains("\"event\":\"listening\"") {
                let _ = tx.send(line);
            }
        }
    });
    rx.recv_timeout(Duration::from_secs(5))
        .expect("evento listening (flag --max-sessions 0 aceptado)");

    // «Detener listen»: bye por stdin → el proceso debe salir SOLO (código 0).
    use std::io::Write;
    writeln!(stdin, "bye").expect("write bye");
    drop(stdin);

    let deadline = std::time::Instant::now() + Duration::from_secs(6);
    let code: Option<i32> = loop {
        match child.try_wait().expect("try_wait") {
            Some(st) => break st.code(),
            None => {
                if std::time::Instant::now() > deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    };
    assert_eq!(
        code,
        Some(0),
        "el bye de stdin debe terminar el proceso con 0"
    );

    // Resumen de cierre legible (fix cosmético: antes salía `Nonems`).
    let mut err = String::new();
    use std::io::Read;
    let _ = stderr.read_to_string(&mut err);
    assert!(
        err.contains("sesión cerrada: heartbeats="),
        "resumen: {err}"
    );
    assert!(!err.contains("Nonems"), "rtt_min formateado: {err}");

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// Regresión del cuelgue de campo: con stdin ABIERTO y sin datos (como la app),
/// el proceso DEBE terminar solo al agotar `--max-secs`. Antes quedaba colgado
/// porque el lector de stdin vivía en el bloqueo de tokio y el derrumbe del
/// runtime esperaba a esa tarea (reproducido: vivo >6 s tras el timeout).
#[test]
fn listener_process_exits_after_timeout_with_open_stdin() {
    let cfg_dir = std::env::temp_dir().join(format!("anyremote-procexit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cfg_dir);
    let bin = env!("CARGO_BIN_EXE_anyremoted");
    let mut child = std::process::Command::new(bin)
        .args([
            "listen",
            "--bind",
            "127.0.0.1:0",
            "--max-secs",
            "1",
            "--video",
            "off",
            "--audio",
            "off",
            "--clipboard",
            "off",
            "--config-dir",
            cfg_dir.to_str().expect("utf8"),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn anyremoted");
    // stdin ABIERTO y sin datos (igual que la app): NO se escribe `bye`.
    let stdin = child.stdin.take().expect("stdin piped");

    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    let code: Option<i32> = loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => break status.code(),
            None => {
                if std::time::Instant::now() > deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    };
    drop(stdin);
    let _ = std::fs::remove_dir_all(&cfg_dir);
    assert_eq!(
        code,
        Some(0),
        "el proceso DEBE terminar solo tras --max-secs con stdin abierto \
         (antes se colgaba en el runtime de tokio esperando leer stdin)"
    );
}
