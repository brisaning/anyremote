//! Gate RF-02a (entrada en sesión): el controlador escribe `{"input":...}` por
//! stdin → el evento viaja por ARSP → el controlado lo valida contra la
//! concesión efectiva (D-07) y lo inyecta con el backend `Recording`
//! (registra sin tocar el sistema real). Cubre: concesión parcial, denegación
//! por `--allow-input` ausente y cierre ordenado tras los eventos.

use std::path::{Path, PathBuf};

use anyremote_daemon::{run_connect, run_listen, ClipboardMode, DaemonConfig, InputBackend};

fn temp_cfg_dir(tag: &str) -> PathBuf {
    // Cert-store compartido: ambos roles usan el MISMO dir (v0.1 localhost).
    let d = std::env::temp_dir().join(format!("anyremote-input-{tag}-{}", std::process::id()));
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

/// Lado controlado: `listen` con la concesión y los flags de entrada indicados.
/// C-45: `allow_input` es la **capacidad**; `input_on_start` el **estado**
/// inicial (la sesión arranca en «solo ver» si es `false`).
fn listen_cfg(
    cfg_dir: PathBuf,
    port: u16,
    grant: &[&str],
    allow_input: bool,
    input_on_start: bool,
) -> DaemonConfig {
    let mut c = DaemonConfig::new(cfg_dir).insecure_accept_any();
    c.bind = format!("127.0.0.1:{port}");
    c.clipboard = ClipboardMode::Off;
    c.grant = grant.iter().map(|s| s.to_string()).collect();
    c.allow_input = allow_input;
    c.input_on_start = input_on_start;
    c.input_backend = InputBackend::Recording; // nunca toca el sistema en tests
    c.max_secs = 6;
    c
}

/// Lado controlador: `connect` con la concesión y las líneas de stdin precargadas
/// (el `bye` final cierra la sesión tras los eventos de entrada).
async fn controller(
    cfg_dir: PathBuf,
    port: u16,
    grant: &[&str],
    lines: &[&str],
) -> anyremote_daemon::RunOutcome {
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(8);
    for l in lines {
        tx.send((*l).to_string()).await.expect("encolar línea");
    }
    let mut c = DaemonConfig::new(cfg_dir).insecure_accept_any();
    c.target = Some(format!("127.0.0.1:{port}"));
    c.clipboard = ClipboardMode::Off;
    c.grant = grant.iter().map(|s| s.to_string()).collect();
    c.bye_source = Some(rx);
    c.max_secs = 5;
    run_connect(c).await.expect("controller ok")
}

/// Lectura de TODOS los JSONL de auditoría del directorio compartido.
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
    }
    kinds.join(",")
}

const KEY_A: &str = r#"{"input":{"key":{"code":0,"ch":"a","down":true}}}"#;
const POINTER: &str = r#"{"input":{"pointer":{"x":320,"y":180,"absolute":true}}}"#;
const BTN_LEFT: &str = r#"{"input":{"button":{"button":0,"down":true}}}"#;
const SCROLL: &str = r#"{"input":{"button":{"wheel":-1}}}"#;

/// Concesión parcial `["keyboard"]`: la tecla se inyecta, el puntero se deniega
/// (falta `mouse`) y la sesión sigue viva hasta el `bye`.
#[tokio::test]
async fn input_partial_grant_injects_keyboard_and_denies_pointer() {
    let cfg_dir = temp_cfg_dir("parcial");
    let port = free_port().await;

    let listen_dir = cfg_dir.clone();
    let listen = tokio::spawn(async move {
        run_listen(listen_cfg(listen_dir, port, &["keyboard"], true, true)).await
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let ctl = controller(
        cfg_dir.clone(),
        port,
        &["keyboard"],
        &[KEY_A, POINTER, "bye"],
    )
    .await;
    let srv = listen.await.expect("join listener").expect("listener ok");

    // El controlado inyecta la tecla y deniega el puntero (D-07 por permiso).
    let ev = srv.events.join("\n");
    assert!(
        ev.contains("\"event\":\"input_injected\"")
            && ev.contains("\"input\":\"KeyDown(Char('a'))\""),
        "falta inyección de la tecla:\n{ev}"
    );
    assert!(
        ev.contains("\"event\":\"input_denied\"") && ev.contains("\"perm\":\"mouse\""),
        "falta denegación del puntero por falta de `mouse`:\n{ev}"
    );
    assert!(
        ev.contains("\"event\":\"closed\"") && ev.contains("\"reason\":\"bye\""),
        "la sesión debe cerrar con el bye posterior:\n{ev}"
    );
    // El controlador solo ve el cierre ordenado (la inyección la audita el controlado).
    let ctl_ev = ctl.events.join("\n");
    assert!(
        ctl_ev.contains("\"event\":\"closed\""),
        "controlador sin closed:\n{ctl_ev}"
    );

    // Auditoría (C7): ambos eventos quedan registrados en JSONL.
    let kinds = audit_kinds(&cfg_dir);
    assert!(kinds.contains("InputInjected"), "audit: {kinds}");
    assert!(kinds.contains("InputDenied"), "audit: {kinds}");

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// Sin `--allow-input` (D-07 por defecto): TODO evento de entrada se deniega
/// aunque los permisos estén concedidos, y la sesión sigue hasta el `bye`.
#[tokio::test]
async fn input_denied_by_default_without_allow_input() {
    let cfg_dir = temp_cfg_dir("sin-flag");
    let port = free_port().await;

    let listen_dir = cfg_dir.clone();
    let listen = tokio::spawn(async move {
        run_listen(listen_cfg(
            listen_dir,
            port,
            &["keyboard", "mouse"],
            false,
            false,
        ))
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let ctl = controller(
        cfg_dir.clone(),
        port,
        &["keyboard", "mouse"],
        &[KEY_A, BTN_LEFT, "bye"],
    )
    .await;
    let srv = listen.await.expect("join listener").expect("listener ok");

    let ev = srv.events.join("\n");
    assert!(
        ev.contains("\"event\":\"input_denied\"") && ev.contains("\"allow_input\":false"),
        "debe denegar con allow_input=false:\n{ev}"
    );
    assert!(
        !ev.contains("\"event\":\"input_injected\""),
        "nada debe inyectarse sin --allow-input:\n{ev}"
    );
    assert!(
        ev.contains("\"event\":\"closed\"") && ev.contains("\"reason\":\"bye\""),
        "{ev}"
    );
    assert!(ctl.events.join("\n").contains("\"event\":\"closed\""));

    let kinds = audit_kinds(&cfg_dir);
    assert!(kinds.contains("InputDenied"), "audit: {kinds}");
    assert!(
        !kinds.contains("InputInjected"),
        "audit sin inyecciones: {kinds}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// Concesión completa + `--allow-input`: teclado, puntero, botón y rueda se
/// inyectan con el backend `Recording` y la sesión cierra ordenadamente.
#[tokio::test]
async fn input_full_grant_injects_keyboard_pointer_button_scroll() {
    let cfg_dir = temp_cfg_dir("completo");
    let port = free_port().await;

    let listen_dir = cfg_dir.clone();
    let listen = tokio::spawn(async move {
        run_listen(listen_cfg(
            listen_dir,
            port,
            &["keyboard", "mouse"],
            true,
            true,
        ))
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let ctl = controller(
        cfg_dir.clone(),
        port,
        &["keyboard", "mouse"],
        &[KEY_A, POINTER, BTN_LEFT, SCROLL, "bye"],
    )
    .await;
    let srv = listen.await.expect("join listener").expect("listener ok");

    let ev = srv.events.join("\n");
    for expected in [
        "\"input\":\"KeyDown(Char('a'))\"",
        "\"input\":\"MoveTo { x: 320, y: 180 }\"",
        "\"input\":\"ButtonDown(Left)\"",
        "\"input\":\"Scroll { delta: -1 }\"",
    ] {
        assert!(ev.contains(expected), "falta {expected}:\n{ev}");
    }
    let injected = ev.matches("\"event\":\"input_injected\"").count();
    assert_eq!(injected, 4, "cuatro inyecciones esperadas:\n{ev}");
    assert!(
        !ev.contains("\"event\":\"input_denied\""),
        "nada debe denegarse:\n{ev}"
    );
    assert!(
        ev.contains("\"event\":\"closed\"") && ev.contains("\"reason\":\"bye\""),
        "{ev}"
    );
    assert!(ctl.events.join("\n").contains("\"event\":\"closed\""));

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// C-45 (F-02): la **capacidad** `--allow-input` no concede nada por sí sola.
/// La sesión arranca en «solo ver» (`input_on_start = false`) y el primer
/// `{"perm":{"input":true}}` del anfitrión la enciende en caliente.
///
/// Secuencia dirigida por eventos (sin sleeps fijos): sesión activa → tecla 'a'
/// (denegada por `toggle_apagado`) → permiso `input:true` → tecla 'b'
/// (inyectada) → `bye`. También comprueba que `listening` publica el estado
/// inicial real (`input_initial=false` con la capacidad armada).
#[tokio::test]
async fn input_off_by_default_with_capability_and_enabled_by_perm() {
    use std::sync::{Arc, Mutex};

    async fn wait_for(sink: &Arc<Mutex<Vec<String>>>, needle: &str) {
        for _ in 0..200 {
            if sink.lock().unwrap().iter().any(|l| l.contains(needle)) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!(
            "timeout esperando `{needle}` en los eventos del anfitrión: {:?}",
            sink.lock().unwrap()
        );
    }

    const KEY_B: &str = r#"{"input":{"key":{"code":0,"ch":"b","down":true}}}"#;

    let cfg_dir = temp_cfg_dir("optin");
    let port = free_port().await;

    let (tx_listen, rx_listen) = tokio::sync::mpsc::channel::<String>(8);
    let listen_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = listen_events.clone();
    let listen = {
        let mut c = listen_cfg(cfg_dir.clone(), port, &["keyboard"], true, false);
        c.bye_source = Some(rx_listen);
        c.on_event = Some(Box::new(move |line| sink.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };

    let (tx_ctl, rx_ctl) = tokio::sync::mpsc::channel::<String>(8);
    let ctl = {
        let mut c = DaemonConfig::new(cfg_dir.clone()).insecure_accept_any();
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Off;
        c.grant = vec!["keyboard".into()];
        c.bye_source = Some(rx_ctl);
        c.max_secs = 5;
        tokio::spawn(async move { run_connect(c).await })
    };

    // 1) Sesión activa → 2) tecla con el toggle apagado → 3) la denegación
    // llega al anfitrión → 4) el anfitrión enciende el control remoto.
    wait_for(&listen_events, "\"event\":\"session_active\"").await;
    tx_ctl.send(KEY_A.to_string()).await.expect("key a");
    wait_for(&listen_events, "\"event\":\"input_denied\"").await;
    tx_listen
        .send(r#"{"perm":{"input":true,"audio":false,"mic":false}}"#.to_string())
        .await
        .expect("permiso input:true");
    wait_for(&listen_events, "\"event\":\"permissions_updated\"").await;

    // 5) La siguiente tecla sí se inyecta → 6) cierre ordenado.
    tx_ctl.send(KEY_B.to_string()).await.expect("key b");
    wait_for(&listen_events, "\"event\":\"input_injected\"").await;
    tx_ctl.send("bye".to_string()).await.expect("bye");

    let ctl = ctl.await.expect("join controller").expect("controller ok");
    let srv = listen.await.expect("join listener").expect("listener ok");
    let ev = srv.events.join("\n");

    assert!(
        ev.contains("\"event\":\"listening\"")
            && ev.contains("\"input_capability\":true")
            && ev.contains("\"input_initial\":false"),
        "`listening` debe publicar capacidad armada y estado inicial apagado:\n{ev}"
    );
    assert_eq!(
        ev.matches("\"event\":\"input_denied\"").count(),
        1,
        "solo la tecla previa al permiso se deniega:\n{ev}"
    );
    assert!(
        ev.contains("\"reason\":\"toggle_apagado\"")
            && ev.contains("\"input_on\":false")
            && ev.contains("\"allow_input\":true"),
        "la denegación distingue capacidad (true) de estado (false):\n{ev}"
    );
    assert_eq!(
        ev.matches("\"event\":\"input_injected\"").count(),
        1,
        "solo la tecla posterior al permiso se inyecta:\n{ev}"
    );
    assert!(
        ev.contains("\"input\":\"KeyDown(Char('b'))\""),
        "la inyectada debe ser 'b':\n{ev}"
    );
    assert!(
        ev.contains("\"event\":\"permissions_updated\"") && ev.contains("\"input\":true"),
        "el permiso se aplicó en caliente:\n{ev}"
    );

    // Acuses al controlador (RF-02a): 1 denegada + 1 inyectada.
    let acks: Vec<&String> = ctl
        .events
        .iter()
        .filter(|l| l.contains("\"event\":\"input_result\""))
        .collect();
    let oks = acks.iter().filter(|l| l.contains("\"ok\":true")).count();
    let denied = acks.iter().filter(|l| l.contains("\"ok\":false")).count();
    assert_eq!((acks.len(), oks, denied), (2, 1, 1), "acuses: {acks:?}");

    let kinds = audit_kinds(&cfg_dir);
    assert!(
        kinds.contains("InputDenied") && kinds.contains("InputInjected"),
        "auditoría: {kinds}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}
