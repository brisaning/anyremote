//! C-12: **dueño ausente** — si el gestor (la app) muere, el daemon detecta
//! el EOF de su stdin y se detiene en lugar de quedar huérfano.
//!
//! Regresión de campo: cerrar la app con «Escuchar (red)» activo dejaba el
//! listener vivo (en Windows cerrar la ventana NO ejecuta `dispose` de la
//! app, y con `--max-sessions 0` la escucha era indefinida).
//!
//! Regla: el EOF **con líneas leídas** (gestor visto) → línea sintética
//! `dueño ausente (stdin cerrado)` que los caminos existentes interpretan
//! como parada; EOF **sin líneas** (stdin nulo/despreocupado) → NO detiene
//! nada (modo desatendido y tests, que nunca escriben en su propio stdin).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyremote_daemon::{run_connect, run_listen, ClipboardMode, DaemonConfig};

fn mk(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("anyremote-owner-{tag}-{}", std::process::id()));
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

/// Proceso real: el dueño escribe UNA línea (como la app con `{"perm"...}`)
/// y cierra su pipe (la app murió) → el daemon debe salir SOLO (código 0).
#[test]
fn listener_process_stops_when_owner_closes_stdin() {
    let cfg_dir = mk("cierra");
    let bin = env!("CARGO_BIN_EXE_anyremoted");
    let mut child = std::process::Command::new(bin)
        .args([
            "listen",
            "--bind",
            "127.0.0.1:0",
            "--max-secs",
            "30", // si el fix falla, el test agota su espera y FALLA
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
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn anyremoted");
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");

    // Espera `listening` leyendo stdout en un hilo (no llenar el pipe).
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let out_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let ol = out_lines.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
        {
            if line.contains("\"event\":\"listening\"") {
                let _ = tx.send(line.clone());
            }
            ol.lock().unwrap().push(line);
        }
    });
    rx.recv_timeout(Duration::from_secs(5))
        .expect("evento listening");

    // La app escribe su línea de gestión/permisos y LUEGO muere (pipe → EOF).
    use std::io::Write;
    writeln!(
        stdin,
        r#"{{"perm":{{"input":true,"audio":false,"mic":false}}}}"#
    )
    .expect("perm");
    drop(stdin); // el dueño desapareció

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
        "el listener DEBE salir solo al morir el dueño (EOF con gestor visto)"
    );
    let text = out_lines.lock().unwrap().join("\n");
    assert!(
        text.contains("dueño ausente (stdin cerrado)"),
        "el cierre se audita con el motivo del dueño ausente:\n{text}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// Regla contraria (discriminador): EOF **sin ninguna línea** = stdin
/// despreocupado (nulo/desatendido) → NO se detiene; vive hasta `--max-secs`.
#[test]
fn detached_stdin_eof_does_not_stop_the_listener() {
    let cfg_dir = mk("desatendido");
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
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn anyremoted");
    let stdout = child.stdout.take().expect("stdout");
    // EOF inmediato SIN escribir nada (count = 0) → modo desatendido.
    drop(child.stdin.take().expect("stdin"));

    let out_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let ol = out_lines.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
        {
            ol.lock().unwrap().push(line);
        }
    });

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
    assert_eq!(code, Some(0), "vive hasta su propio --max-secs");
    let text = out_lines.lock().unwrap().join("\n");
    assert!(
        text.contains("listen_timeout"),
        "termina por timeout, NO por dueño ausente:\n{text}"
    );
    assert!(
        !text.contains("dueño ausente"),
        "EOF sin líneas NO debe detener:\n{text}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// En proceso: la pérdida del dueño DURANTE una sesión con repetidor
/// (`max_sessions = 0`) cierra la sesión con el motivo y DETIENE la escucha
/// (no vuelve a aceptar), y el par recibe el `Bye` con ese motivo.
#[tokio::test]
async fn owner_loss_during_session_closes_it_and_the_repeat_listener() {
    let cfg_dir = mk("sesion");
    let port = free_port().await;

    let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
    let listen_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let le = listen_events.clone();
    let listen = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.bind = format!("127.0.0.1:{port}");
        c.clipboard = ClipboardMode::Off;
        c.max_secs = 20;
        c.max_sessions = 0; // repetidor: solo el dueño lo puede parar
        c.bye_source = Some(rx);
        c.on_event = Some(Box::new(move |line: String| le.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;

    let ctl_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let ce = ctl_events.clone();
    let ctl = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Off;
        c.max_secs = 8; // red de seguridad del test
        c.on_event = Some(Box::new(move |line: String| ce.lock().unwrap().push(line)));
        tokio::spawn(async move { run_connect(c).await })
    };
    tokio::time::sleep(Duration::from_millis(600)).await; // sesión activa

    // Lo que el lector de stdin enviaría al morir la app:
    tx.send("dueño ausente (stdin cerrado)".to_string())
        .await
        .expect("envía aviso de dueño ausente");

    let srv = tokio::time::timeout(Duration::from_secs(5), listen)
        .await
        .expect("el listener se detiene (no re-acepta)")
        .expect("join listener")
        .expect("listener ok");
    let ctl = tokio::time::timeout(Duration::from_secs(8), ctl)
        .await
        .expect("la sesión del par se cierra")
        .expect("join controller")
        .expect("controller ok");
    let _ = ctl;

    let text = srv.events.join("\n");
    assert!(
        text.contains("\"reason\":\"dueño ausente (stdin cerrado)\""),
        "cierre de sesión + escucha con el motivo:\n{text}"
    );
    let ce = ctl_events.lock().unwrap().join("\n");
    assert!(
        ce.contains("dueño ausente (stdin cerrado)"),
        "el par recibe el Bye con el motivo:\n{ce}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}
