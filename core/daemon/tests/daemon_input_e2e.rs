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

/// Lado controlado: `listen` con la concesión y el flag de entrada indicados.
fn listen_cfg(cfg_dir: PathBuf, port: u16, grant: &[&str], allow_input: bool) -> DaemonConfig {
    let mut c = DaemonConfig::new(cfg_dir);
    c.bind = format!("127.0.0.1:{port}");
    c.clipboard = ClipboardMode::Off;
    c.grant = grant.iter().map(|s| s.to_string()).collect();
    c.allow_input = allow_input;
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
    let mut c = DaemonConfig::new(cfg_dir);
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
    let listen =
        tokio::spawn(
            async move { run_listen(listen_cfg(listen_dir, port, &["keyboard"], true)).await },
        );
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
        run_listen(listen_cfg(listen_dir, port, &["keyboard", "mouse"], false)).await
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
        run_listen(listen_cfg(listen_dir, port, &["keyboard", "mouse"], true)).await
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
