//! Gate C-44 — **autorización del par** (F-01/RNF-07).
//!
//! La autenticación C2 demuestra posesión de una clave, pero la sesión solo se
//! activa con una decisión del anfitrión:
//! - `trusted`: solo pares confiados (`--trust` o `trusted.ids`);
//! - `ask`: el anfitrión decide por stdin (`{"accept":"<ID>"}` / `{"reject":...}`);
//! - `any`: comportamiento histórico (explícito; los demás e2e lo usan).
//!
//! También fija la inversión del grant: el `PermissionResponse` del par es
//! intención, no autorización (el anfitrión concede solo lo que él pidió).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyremote_daemon::{
    run_connect, run_listen, AcceptPolicy, ClipboardMode, DaemonConfig, InputBackend,
};

fn temp_cfg_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("anyremote-authz-{tag}-{}", std::process::id()));
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

fn base_cfg(cfg_dir: PathBuf, port: u16) -> DaemonConfig {
    let mut c = DaemonConfig::new(cfg_dir);
    c.bind = format!("127.0.0.1:{port}");
    c.clipboard = ClipboardMode::Off;
    c.input_backend = InputBackend::Recording;
    c.grant = vec!["keyboard".into(), "mouse".into()];
    c.allow_input = true;
    c.max_secs = 8;
    c
}

type Sink = Arc<Mutex<Vec<String>>>;

async fn wait_for_line(sink: &Sink, needle: &str) -> String {
    for _ in 0..240 {
        if let Some(line) = sink.lock().unwrap().iter().find(|l| l.contains(needle)) {
            return line.clone();
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!(
        "timeout esperando `{needle}`; eventos: {:?}",
        sink.lock().unwrap()
    );
}

/// Espera a que un evento aparezca al menos `n` veces (varias sesiones del
/// mismo listener, C-54). Espera más que `wait_for_line`: el listener atiende
/// las sesiones de una en una.
async fn wait_for_count(sink: &Sink, needle: &str, n: usize) {
    for _ in 0..400 {
        if sink
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.contains(needle))
            .count()
            >= n
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!(
        "timeout esperando {n} × `{needle}`; eventos: {:?}",
        sink.lock().unwrap()
    );
}

fn count_events(sink: &Sink, needle: &str) -> usize {
    sink.lock()
        .unwrap()
        .iter()
        .filter(|l| l.contains(needle))
        .count()
}

fn peer_of(pending_line: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(pending_line).expect("json de session_pending");
    v["peer"]
        .as_str()
        .expect("peer en session_pending")
        .to_string()
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
    }
    kinds.join(",")
}

fn audit_text(cfg_dir: &Path) -> String {
    let mut files = std::fs::read_dir(cfg_dir.join("audit"))
        .expect("audit dir")
        .flatten()
        .map(|e| e.path())
        .collect::<Vec<_>>();
    files.sort();
    files
        .iter()
        .map(|f| std::fs::read_to_string(f).expect("leer audit"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Un par NUNCA visto es rechazado por defecto (`trusted`), sin sesión activa
/// ni inyección, y el listener sigue aceptando.
#[tokio::test]
async fn unknown_peer_rejected_by_default() {
    let cfg_dir = temp_cfg_dir("unknown");
    let port = free_port().await;

    let (tx_listen, rx_listen) = tokio::sync::mpsc::channel::<String>(8);
    let sink: Sink = Arc::new(Mutex::new(Vec::new()));
    let s = sink.clone();
    let listen = {
        let mut c = base_cfg(cfg_dir.clone(), port);
        c.accept = AcceptPolicy::Trusted; // default del binario/app antes de C-44
        c.bye_source = Some(rx_listen);
        c.on_event = Some(Box::new(move |line| s.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(120)).await;

    // El controlador comparte el cert-store (v0.1 localhost) pero es un par
    // desconocido para el trust store: su C2 es válido y aun así se le deniega.
    let mut ctl = DaemonConfig::new(cfg_dir.clone());
    ctl.target = Some(format!("127.0.0.1:{port}"));
    ctl.clipboard = ClipboardMode::Off;
    ctl.max_secs = 4;
    let err = match run_connect(ctl).await {
        Ok(_) => panic!("el par desconocido no debe completar la sesión"),
        Err(e) => e,
    };
    assert!(
        format!("{err:?}").to_lowercase().contains("eof")
            || format!("{err:?}").to_lowercase().contains("10054")
            || format!("{err:?}").to_lowercase().contains("closed")
            || format!("{err:?}").to_lowercase().contains("reset"),
        "el controlador debe ver el cierre del handshake: {err:?}"
    );

    // El listener sigue vivo: se detiene con `bye` (no por el rechazo).
    tx_listen.send("bye".to_string()).await.expect("bye");
    let srv = listen.await.expect("join listener").expect("listener ok");
    let ev = srv.events.join("\n");

    assert!(
        ev.contains("\"event\":\"peer_rejected\"")
            && ev.contains("\"reason\":\"peer_unknown\"")
            && ev.contains("\"event\":\"listening\""),
        "debe auditar/emitir el rechazo y seguir escuchando:\n{ev}"
    );
    assert!(
        !ev.contains("\"event\":\"session_active\""),
        "un par desconocido no puede activar sesión:\n{ev}"
    );
    assert!(
        !ev.contains("\"event\":\"input_injected\""),
        "nada de entrada sin sesión autorizada:\n{ev}"
    );
    assert!(
        !ev.contains("\"event\":\"permission_granted\""),
        "no se conceden permisos a un par no autorizado:\n{ev}"
    );
    let kinds = audit_kinds(&cfg_dir);
    assert!(kinds.contains("PermissionDenied"), "audit: {kinds}");
    assert!(
        !kinds.contains("SessionEstablished"),
        "audit sin sesión establecida: {kinds}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// `--trust <ID>` pre-autoriza al par: la sesión se activa y queda recordado
/// en `trusted.ids` para la próxima vez.
#[tokio::test]
async fn trusted_peer_gets_active() {
    let cfg_dir = temp_cfg_dir("trusted");
    let port = free_port().await;
    // Ambos roles comparten config-dir en los e2e (cert-store local): el ID
    // del par es el de la identidad persistida allí.
    let peer_id = anyremote_identity::Identity::load_or_generate(&cfg_dir)
        .expect("identidad")
        .node_id()
        .to_string();

    let (_tx_listen, rx_listen) = tokio::sync::mpsc::channel::<String>(8);
    let listen = {
        let mut c = base_cfg(cfg_dir.clone(), port);
        c.accept = AcceptPolicy::Trusted;
        c.trusted_ids = vec![peer_id.clone()];
        c.bye_source = Some(rx_listen);
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(120)).await;

    let ctl = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Off;
        c.max_secs = 4;
        tokio::spawn(async move { run_connect(c).await })
    };

    let ctl = ctl.await.expect("join controller").expect("controller ok");
    assert!(
        ctl.events
            .iter()
            .any(|e| e.contains("\"event\":\"session_active\"")),
        "el controlador debe activar la sesión: {:?}",
        ctl.events
    );
    // max_sessions=1: la sesión completada detiene el listener (sin bye extra).
    let srv = listen.await.expect("join listener").expect("listener ok");
    let ev = srv.events.join("\n");
    assert!(
        ev.contains("\"event\":\"session_active\"") && ev.contains("\"peer_trusted\":false"),
        "primera sesión pre-autorizada (aún sin TOFU):\n{ev}"
    );
    // La aceptación se recuerda: la próxima entrará sin `--trust`.
    let trust = std::fs::read_to_string(cfg_dir.join("trusted.ids")).expect("trusted.ids");
    assert!(
        trust.contains(&peer_id),
        "el par aceptado debe quedar en trusted.ids:\n{trust}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// `--accept ask`: la sesión queda PENDIENTE (sin `session_active` ni
/// `session_established`) hasta que el anfitrión decide por stdin.
#[tokio::test]
async fn ask_mode_blocks_until_host_decision() {
    let cfg_dir = temp_cfg_dir("ask-accept");
    let port = free_port().await;

    let (tx_listen, rx_listen) = tokio::sync::mpsc::channel::<String>(8);
    let sink: Sink = Arc::new(Mutex::new(Vec::new()));
    let s = sink.clone();
    let listen = {
        let mut c = base_cfg(cfg_dir.clone(), port);
        c.accept = AcceptPolicy::Ask;
        c.accept_timeout_secs = 5;
        c.bye_source = Some(rx_listen);
        c.on_event = Some(Box::new(move |line| s.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(120)).await;

    let ctl = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Off;
        c.max_secs = 4;
        tokio::spawn(async move { run_connect(c).await })
    };

    // Pendiente y bloqueado: no hay sesión hasta la decisión.
    let pending = wait_for_line(&sink, "\"event\":\"session_pending\"").await;
    assert!(
        !sink
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains("\"event\":\"session_active\"")),
        "no debe haber sesión activa antes de decidir: {:?}",
        sink.lock().unwrap()
    );
    let peer = peer_of(&pending);
    tx_listen
        .send(format!("{{\"accept\":\"{peer}\"}}"))
        .await
        .expect("accept");

    let ctl = ctl.await.expect("join controller").expect("controller ok");
    assert!(
        ctl.events
            .iter()
            .any(|e| e.contains("\"event\":\"session_active\"")),
        "tras aceptar, el controlador activa: {:?}",
        ctl.events
    );
    // max_sessions=1: la sesión completada detiene el listener (sin bye extra).
    let srv = listen.await.expect("join listener").expect("listener ok");
    let ev = srv.events.join("\n");
    assert!(
        ev.contains("\"event\":\"session_active\""),
        "la sesión se activa tras la decisión:\n{ev}"
    );
    let audit = audit_text(&cfg_dir);
    assert!(
        audit.contains("\"kind\":\"permission_granted\"")
            && audit.contains("\"source\":\"host\"")
            && audit.contains("\"perms\":[\"keyboard\""),
        "la concesión la decide el anfitrión y queda auditada:\n{audit}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// `--accept ask`: la decisión «rechazar» cierra el cliente y el listener sigue.
#[tokio::test]
async fn ask_mode_rejects_on_host_decision() {
    let cfg_dir = temp_cfg_dir("ask-reject");
    let port = free_port().await;

    let (tx_listen, rx_listen) = tokio::sync::mpsc::channel::<String>(8);
    let sink: Sink = Arc::new(Mutex::new(Vec::new()));
    let s = sink.clone();
    let listen = {
        let mut c = base_cfg(cfg_dir.clone(), port);
        c.accept = AcceptPolicy::Ask;
        c.accept_timeout_secs = 5;
        c.bye_source = Some(rx_listen);
        c.on_event = Some(Box::new(move |line| s.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(120)).await;

    let ctl = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Off;
        c.max_secs = 4;
        tokio::spawn(async move { run_connect(c).await })
    };

    let pending = wait_for_line(&sink, "\"event\":\"session_pending\"").await;
    let peer = peer_of(&pending);
    tx_listen
        .send(format!("{{\"reject\":\"{peer}\"}}"))
        .await
        .expect("reject");

    let rejected = ctl.await.expect("join controller");
    assert!(
        rejected.is_err(),
        "el controlador no debe activar tras el rechazo"
    );
    tx_listen.send("bye".to_string()).await.expect("bye");
    let srv = listen.await.expect("join listener").expect("listener ok");
    let ev = srv.events.join("\n");
    assert!(
        ev.contains("\"event\":\"peer_rejected\"")
            && ev.contains("\"reason\":\"rejected_by_host\""),
        "rechazo del anfitrión auditado:\n{ev}"
    );
    assert!(
        !ev.contains("\"event\":\"session_active\""),
        "sin sesión tras rechazo:\n{ev}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// Regresión de campo (2026-11-19): un ID heredado del TOFU antiguo (formato
/// sin sufijo, guardado por versiones previas sin consentimiento) se carga
/// como `seen` y **no** autoriza en `ask`: el anfitrión debe recibir
/// `session_pending` y decidir. «Aceptar y confiar» lo deja como `user`.
#[tokio::test]
async fn ask_mode_prompts_for_legacy_seen_peer() {
    let cfg_dir = temp_cfg_dir("ask-legacy");
    let port = free_port().await;

    // Identidad compartida (cert-store local) → el par que conectará es este.
    let ident = anyremote_identity::Identity::load_or_generate(&cfg_dir).expect("identidad");
    let peer_id = ident.node_id().to_string();
    let pk_hex = data_encoding::HEXLOWER.encode(&ident.verifying_key_bytes());
    // Formato v1: `ID=hex` (sin `:user|:seen`).
    std::fs::write(
        cfg_dir.join("trusted.ids"),
        format!("# anyremote: pares autenticados (TOFU)\n{peer_id}={pk_hex}\n"),
    )
    .expect("trusted.ids heredado");

    let (tx_listen, rx_listen) = tokio::sync::mpsc::channel::<String>(8);
    let sink: Sink = Arc::new(Mutex::new(Vec::new()));
    let s = sink.clone();
    let listen = {
        let mut c = base_cfg(cfg_dir.clone(), port);
        c.accept = AcceptPolicy::Ask;
        c.accept_timeout_secs = 5;
        c.bye_source = Some(rx_listen);
        c.on_event = Some(Box::new(move |line| s.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(120)).await;

    let ctl = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Off;
        c.max_secs = 4;
        tokio::spawn(async move { run_connect(c).await })
    };

    // Debe PREGUNTAR: la entrada heredada es «vista», no autorización.
    let pending = wait_for_line(&sink, "\"event\":\"session_pending\"").await;
    assert_eq!(peer_of(&pending), peer_id);
    assert_eq!(
        sink.lock()
            .unwrap()
            .iter()
            .filter(|l| l.contains("\"event\":\"session_pending\""))
            .count(),
        1,
        "un solo aviso de sesión pendiente"
    );

    tx_listen
        .send(format!("{{\"accept\":\"{peer_id}\",\"trust\":true}}"))
        .await
        .expect("accept+trust");
    let ctl = ctl.await.expect("join controller").expect("controller ok");
    assert!(
        ctl.events
            .iter()
            .any(|e| e.contains("\"event\":\"session_active\"")),
        "tras decidir, la sesión se activa: {:?}",
        ctl.events
    );
    let srv = listen.await.expect("join listener").expect("listener ok");
    assert!(srv
        .events
        .iter()
        .any(|e| e.contains("\"event\":\"session_active\"")));

    // La decisión explícita queda como `user` (no vuelve a preguntar).
    let trust = std::fs::read_to_string(cfg_dir.join("trusted.ids")).expect("trusted.ids");
    assert!(
        trust.contains(&format!("{peer_id}={pk_hex}:user")),
        "«Aceptar y confiar» debe persistir user:\n{trust}"
    );
    // C-54: una sola entrada por par aunque los reintentos reutilicen la
    // decisión (`TrustStore::remember` es idempotente).
    assert_eq!(
        trust.lines().filter(|l| l.contains(&peer_id)).count(),
        1,
        "trusted.ids no debe duplicar el par:\n{trust}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// C-44/C-45 (v0.14.3): un par `user` con el control **encendido** entra sin
/// diálogo; el estado del toggle persiste entre sesiones (no vuelve a «solo
/// ver» al reconectar).
#[tokio::test]
async fn ask_mode_auto_accepts_user_trusted_peer_when_control_is_on() {
    let cfg_dir = temp_cfg_dir("ask-trusted-on");
    let port = free_port().await;
    let ident = anyremote_identity::Identity::load_or_generate(&cfg_dir).expect("identidad");
    let peer_id = ident.node_id().to_string();
    let pk_hex = data_encoding::HEXLOWER.encode(&ident.verifying_key_bytes());
    std::fs::write(
        cfg_dir.join("trusted.ids"),
        format!("# anyremote: pares conocidos\n{peer_id}={pk_hex}:user\n"),
    )
    .expect("trusted.ids user");

    let sink: Sink = Arc::new(Mutex::new(Vec::new()));
    let s = sink.clone();
    let listen = {
        let mut c = base_cfg(cfg_dir.clone(), port);
        c.accept = AcceptPolicy::Ask;
        c.set_input_on(true); // toggle del anfitrión encendido (persistido)
        c.max_secs = 2;
        c.on_event = Some(Box::new(move |line| s.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(120)).await;

    let ctl = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Off;
        c.max_secs = 2;
        tokio::spawn(async move { run_connect(c).await })
    };

    wait_for_line(&sink, "\"event\":\"session_active\"").await;
    assert!(
        !sink
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains("\"event\":\"session_pending\"")),
        "un par de confianza con control ON no debe volver a preguntar: {:?}",
        sink.lock().unwrap()
    );
    ctl.await.expect("join controller").expect("controller ok");
    listen.await.expect("join listener").expect("listener ok");

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// C-44/C-45 (v0.14.3): si el control está **apagado**, incluso un par `user`
/// vuelve a preguntar (el diálogo es donde se concede el control).
#[tokio::test]
async fn ask_mode_prompts_trusted_peer_when_control_is_off() {
    let cfg_dir = temp_cfg_dir("ask-trusted-off");
    let port = free_port().await;
    let ident = anyremote_identity::Identity::load_or_generate(&cfg_dir).expect("identidad");
    let peer_id = ident.node_id().to_string();
    let pk_hex = data_encoding::HEXLOWER.encode(&ident.verifying_key_bytes());
    std::fs::write(
        cfg_dir.join("trusted.ids"),
        format!("# anyremote: pares conocidos\n{peer_id}={pk_hex}:user\n"),
    )
    .expect("trusted.ids user");

    let (tx_listen, rx_listen) = tokio::sync::mpsc::channel::<String>(8);
    let sink: Sink = Arc::new(Mutex::new(Vec::new()));
    let s = sink.clone();
    let listen = {
        let mut c = base_cfg(cfg_dir.clone(), port);
        c.accept = AcceptPolicy::Ask;
        c.accept_timeout_secs = 5;
        c.bye_source = Some(rx_listen);
        c.on_event = Some(Box::new(move |line| s.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(120)).await;

    let ctl = {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Off;
        c.max_secs = 2;
        tokio::spawn(async move { run_connect(c).await })
    };

    let pending = wait_for_line(&sink, "\"event\":\"session_pending\"").await;
    let peer = peer_of(&pending);
    tx_listen
        .send(format!("{{\"accept\":\"{peer}\"}}"))
        .await
        .expect("accept");
    let ctl = ctl.await.expect("join controller").expect("controller ok");
    assert!(
        ctl.events
            .iter()
            .any(|e| e.contains("\"event\":\"session_active\"")),
        "tras aceptar, la sesión se activa"
    );
    listen.await.expect("join listener").expect("listener ok");

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// C-54 (fix de campo 2026-11-23): el cliente abre **varios intentos** de la
/// misma conexión (QUIC → TCP → relay). Los intentos de un mismo par comparten
/// UNA decisión del anfitrión: un solo `session_pending` y ambas conexiones se
/// activan (el listener las atiende de una en una; el diálogo no se repite).
#[tokio::test]
async fn ask_mode_prompts_once_per_peer() {
    let cfg_dir = temp_cfg_dir("ask-once");
    let port = free_port().await;

    let (tx_listen, rx_listen) = tokio::sync::mpsc::channel::<String>(8);
    let sink: Sink = Arc::new(Mutex::new(Vec::new()));
    let s = sink.clone();
    let listen = {
        let mut c = base_cfg(cfg_dir.clone(), port);
        c.accept = AcceptPolicy::Ask;
        c.accept_timeout_secs = 10;
        c.accept_decision_ttl_secs = 60;
        // Cada sesión dura 1 s: el segundo intento espera en la cola TCP y su
        // bootstrap del certificado tiene 2 s de presupuesto (transport).
        c.max_secs = 1;
        c.max_sessions = 0; // la escucha sigue viva (modo «Escuchar (red)»)
        c.bye_source = Some(rx_listen);
        c.on_event = Some(Box::new(move |line| s.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(120)).await;

    // Dos intentos del MISMO par (comparten `config_dir` → misma identidad).
    let attempt = |max_secs: u64| {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Off;
        c.max_secs = max_secs;
        tokio::spawn(async move { run_connect(c).await })
    };
    let first = attempt(1);
    let second = attempt(1);

    // Un único aviso aunque haya dos intentos en vuelo.
    let pending = wait_for_line(&sink, "\"event\":\"session_pending\"").await;
    let peer = peer_of(&pending);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        count_events(&sink, "\"event\":\"session_pending\""),
        1,
        "dos intentos del mismo par = un solo diálogo: {:?}",
        sink.lock().unwrap()
    );

    tx_listen
        .send(format!("{{\"accept\":\"{peer}\"}}"))
        .await
        .expect("accept");

    // Ambos intentos llegan a sesión activa con la misma decisión.
    wait_for_count(&sink, "\"event\":\"session_active\"", 2).await;
    for (name, ctl) in [("primero", first), ("segundo", second)] {
        let out = ctl
            .await
            .expect("join controlador")
            .expect("controlador ok");
        assert!(
            out.events
                .iter()
                .any(|e| e.contains("\"event\":\"session_active\"")),
            "el intento {name} debe activarse: {:?}",
            out.events
        );
    }

    tx_listen.send("bye".to_string()).await.expect("bye");
    let srv = listen.await.expect("join listener").expect("listener ok");
    let ev = srv.events.join("\n");
    assert_eq!(
        ev.matches("\"event\":\"session_pending\"").count(),
        1,
        "un solo diálogo para todo el cliente:\n{ev}"
    );
    assert_eq!(
        ev.matches("\"event\":\"session_active\"").count(),
        2,
        "los dos intentos se activan:\n{ev}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// C-54: tras decidir por un par, un intento **nuevo** del mismo par dentro de
/// la ventana (`--accept-decision-ttl`) no vuelve a preguntar: se activa
/// directamente. Antes, este caso costaba otro diálogo (y un timeout si el
/// anfitrión no lo atendía).
#[tokio::test]
async fn ask_mode_reuses_recent_decision() {
    let cfg_dir = temp_cfg_dir("ask-reuse");
    let port = free_port().await;

    let (tx_listen, rx_listen) = tokio::sync::mpsc::channel::<String>(8);
    let sink: Sink = Arc::new(Mutex::new(Vec::new()));
    let s = sink.clone();
    let listen = {
        let mut c = base_cfg(cfg_dir.clone(), port);
        c.accept = AcceptPolicy::Ask;
        c.accept_timeout_secs = 5;
        c.accept_decision_ttl_secs = 60;
        c.max_secs = 1; // las sesiones cierran solas, sin `bye` por intento
        c.max_sessions = 0;
        c.bye_source = Some(rx_listen);
        c.on_event = Some(Box::new(move |line| s.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(120)).await;

    let attempt = |max_secs: u64| {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Off;
        c.max_secs = max_secs;
        tokio::spawn(async move { run_connect(c).await })
    };

    // Primer intento: el anfitrión decide aceptar.
    let first = attempt(1);
    let pending = wait_for_line(&sink, "\"event\":\"session_pending\"").await;
    let peer = peer_of(&pending);
    tx_listen
        .send(format!("{{\"accept\":\"{peer}\"}}"))
        .await
        .expect("accept");
    wait_for_count(&sink, "\"event\":\"session_active\"", 1).await;
    first.await.expect("join 1").expect("controlador 1 ok");

    // Segundo intento dentro de la ventana: sin diálogo nuevo.
    let second = attempt(1);
    wait_for_count(&sink, "\"event\":\"session_active\"", 2).await;
    let out = second.await.expect("join 2").expect("controlador 2 ok");
    assert!(
        out.events
            .iter()
            .any(|e| e.contains("\"event\":\"session_active\"")),
        "el reintento se activa con la decisión reciente: {:?}",
        out.events
    );

    tx_listen.send("bye".to_string()).await.expect("bye");
    let srv = listen.await.expect("join listener").expect("listener ok");
    let ev = srv.events.join("\n");
    assert_eq!(
        ev.matches("\"event\":\"session_pending\"").count(),
        1,
        "la decisión reciente evita un segundo diálogo:\n{ev}"
    );
    assert!(
        ev.contains("\"event\":\"session_decision_reused\""),
        "el camino de reutilización queda trazado:\n{ev}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// C-54: un rechazo también se recuerda (ventana corta) para no preguntar de
/// inmediato al mismo par por cada intento que quede en vuelo.
#[tokio::test]
async fn ask_mode_reuses_recent_rejection() {
    let cfg_dir = temp_cfg_dir("ask-reject-reuse");
    let port = free_port().await;

    let (tx_listen, rx_listen) = tokio::sync::mpsc::channel::<String>(8);
    let sink: Sink = Arc::new(Mutex::new(Vec::new()));
    let s = sink.clone();
    let listen = {
        let mut c = base_cfg(cfg_dir.clone(), port);
        c.accept = AcceptPolicy::Ask;
        c.accept_timeout_secs = 5;
        c.accept_decision_ttl_secs = 60; // rechazo = 30 s
        c.max_sessions = 0;
        c.bye_source = Some(rx_listen);
        c.on_event = Some(Box::new(move |line| s.lock().unwrap().push(line)));
        tokio::spawn(async move { run_listen(c).await })
    };
    tokio::time::sleep(Duration::from_millis(120)).await;

    let attempt = || {
        let mut c = DaemonConfig::new(cfg_dir.clone());
        c.target = Some(format!("127.0.0.1:{port}"));
        c.clipboard = ClipboardMode::Off;
        c.max_secs = 2;
        tokio::spawn(async move { run_connect(c).await })
    };

    let first = attempt();
    let pending = wait_for_line(&sink, "\"event\":\"session_pending\"").await;
    let peer = peer_of(&pending);
    tx_listen
        .send(format!("{{\"reject\":\"{peer}\"}}"))
        .await
        .expect("reject");
    assert!(
        first.await.expect("join 1").is_err(),
        "el primer intento no debe activar"
    );
    wait_for_line(&sink, "\"reason\":\"rejected_by_host\"").await;

    // Segundo intento: se rechaza sin volver a preguntar.
    let second = attempt();
    assert!(
        second.await.expect("join 2").is_err(),
        "el reintento tampoco activa"
    );
    wait_for_line(&sink, "\"reason\":\"rejected_recently\"").await;
    assert_eq!(
        count_events(&sink, "\"event\":\"session_pending\""),
        1,
        "el rechazo reciente no reabre el diálogo: {:?}",
        sink.lock().unwrap()
    );

    tx_listen.send("bye".to_string()).await.expect("bye");
    listen.await.expect("join listener").expect("listener ok");

    let _ = std::fs::remove_dir_all(&cfg_dir);
}
