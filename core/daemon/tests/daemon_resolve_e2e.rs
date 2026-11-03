//! Gate C-16 — conexión por ID (resolutor LAN, `specs/02` §9):
//!
//! 1. `resolves_id_by_discovery_and_sessions` — el controlador resuelve el
//!    ID estable por consulta UDP de descubrimiento (`resolve_ok` con
//!    `source:broadcast`) y completa la sesión contra el anfitrión.
//! 2. `unresolved_id_fails_with_netsh_hint` — sin anfitrión ni caché → error
//!    accionable con el `netsh` del puerto de descubrimiento.
//! 3. `cache_redirect_to_wrong_machine_is_rejected` — una caché que apunta a
//!    una máquina con OTRO id (dirección incorrecta/intermediario) se rechaza
//!    tras C2 **antes** de guardar pin o TOFU (la resolución no autentica).

use std::sync::{Arc, Mutex};

use anyremote_daemon::{run_connect, run_listen, ClipboardMode, DaemonConfig};

fn mk(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("anyremote-resolve-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    d
}

/// Puerto no efímero reservado (patrón de `daemon_quic_e2e`: los efímeros los
/// recicla el SO agresivamente bajo carga de suite completa).
async fn free_port() -> u16 {
    use std::sync::atomic::{AtomicU16, Ordering};
    static NEXT: AtomicU16 = AtomicU16::new(0);
    let off = NEXT.fetch_add(4, Ordering::SeqCst);
    let base = 45_000u16
        .wrapping_add((std::process::id() as u16).wrapping_mul(7) % 4_000)
        .wrapping_add(off);
    for port in [base, base + 1, base + 2, base + 3] {
        let tcp = tokio::net::TcpListener::bind(("127.0.0.1", port)).await;
        let udp = tokio::net::UdpSocket::bind(("127.0.0.1", port)).await;
        if let (Ok(_), Ok(_)) = (tcp, udp) {
            return port;
        }
    }
    panic!("sin puerto libre en {base}..{}", base + 3);
}

/// Espera a que el listener publique `listening` (y devuelve sus eventos).
async fn wait_listening(events: &Arc<Mutex<Vec<String>>>) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if events
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains("\"event\":\"listening\""))
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "el listener no publicó listening: {:?}",
            events.lock().unwrap()
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

fn listen_cfg(
    dir: std::path::PathBuf,
    bind_port: u16,
    disc_port: u16,
    events: Arc<Mutex<Vec<String>>>,
) -> DaemonConfig {
    let mut c = DaemonConfig::new(dir);
    c.bind = format!("127.0.0.1:{bind_port}");
    c.clipboard = ClipboardMode::Off;
    c.discover = format!("127.0.0.1:{disc_port}");
    c.max_secs = 8;
    c.on_event = Some(Box::new(move |l: String| events.lock().unwrap().push(l)));
    c
}

fn connect_cfg(
    dir: std::path::PathBuf,
    target: &str,
    disc_port: u16,
    events: Arc<Mutex<Vec<String>>>,
) -> DaemonConfig {
    let mut c = DaemonConfig::new(dir);
    c.target = Some(target.to_string());
    c.clipboard = ClipboardMode::Off;
    c.discover = format!("127.0.0.1:{disc_port}");
    c.max_secs = 4;
    c.on_event = Some(Box::new(move |l: String| events.lock().unwrap().push(l)));
    c
}

/// Escenario 1: resolución por broadcast + sesión completa por ID.
#[tokio::test]
async fn resolves_id_by_discovery_and_sessions() {
    let cfg_dir = mk("ok");
    let bind_port = free_port().await;
    let disc_port = free_port().await;
    // Misma carpeta que el listener → la identidad compartida (patrón de los
    // e2e existentes); el ID resuelto es el del propio anfitrión.
    let node_id = anyremote_identity::Identity::load_or_generate(&cfg_dir)
        .expect("identidad")
        .node_id()
        .to_string();

    let listen_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let listen = tokio::spawn({
        let cfg_dir = cfg_dir.clone();
        let le = listen_events.clone();
        async move { run_listen(listen_cfg(cfg_dir, bind_port, disc_port, le)).await }
    });
    wait_listening(&listen_events).await;

    let ctl_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let controller = tokio::spawn({
        let cfg_dir = cfg_dir.clone();
        let ce = ctl_events.clone();
        let id = node_id.clone();
        async move { run_connect(connect_cfg(cfg_dir, &id, disc_port, ce)).await }
    });

    let ctl = controller
        .await
        .expect("join ctl")
        .expect("sesión completa por ID");
    let srv = listen
        .await
        .expect("join listener")
        .expect("listener completo");
    let _ = (ctl, srv);

    let ctl_text = ctl_events.lock().unwrap().join("\n");
    let srv_text = listen_events.lock().unwrap().join("\n");
    // Resuelto por descubrimiento vivo (no caché): evento con la dirección.
    assert!(
        ctl_text.contains("\"event\":\"resolve_ok\""),
        "evento resolve_ok:\n{ctl_text}"
    );
    assert!(
        ctl_text.contains("\"source\":\"broadcast\""),
        "resolución por broadcast:\n{ctl_text}"
    );
    assert!(
        ctl_text.contains(&format!("\"id\":\"{node_id}\"")),
        "resolve_ok lleva el id:\n{ctl_text}"
    );
    // Y la sesión de verdad ocurrió contra ese anfitrión.
    assert!(
        srv_text.contains("\"event\":\"session_active\""),
        "sesión establecida:\n{srv_text}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// Escenario 2: sin anfitrión ni caché → error accionable (netsh).
#[tokio::test]
async fn unresolved_id_fails_with_netsh_hint() {
    let cfg_dir = mk("fail");
    let dead_port = free_port().await; // nadie escucha aquí
    let node_id = anyremote_identity::Identity::load_or_generate(&cfg_dir)
        .expect("identidad")
        .node_id()
        .to_string();

    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let controller = tokio::spawn({
        let cfg_dir = cfg_dir.clone();
        let ce = events.clone();
        let id = node_id.clone();
        async move { run_connect(connect_cfg(cfg_dir, &id, dead_port, ce)).await }
    });

    // (RunOutcome no implementa Debug → match explícito en vez de expect_err)
    let err = match controller.await.expect("join ctl") {
        Ok(_) => panic!("sin resolución debe fallar"),
        Err(e) => e,
    };
    let msg = format!("{err:?}");
    assert!(msg.contains("no se pudo resolver"), "mensaje: {msg}");
    assert!(
        msg.contains("anyremote-discover") && msg.contains(&format!("localport={dead_port}")),
        "hint netsh del puerto de descubrimiento: {msg}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// Escenario 3: la resolución NO autentica — una caché que apunta a una
/// máquina con OTRO id se rechaza tras C2, antes de guardar pin/TOFU.
#[tokio::test]
async fn cache_redirect_to_wrong_machine_is_rejected() {
    let cfg_dir = mk("impostor");
    let bind_port = free_port().await;
    let dead_port = free_port().await; // la broadcast no responde → caché
    let real_id = anyremote_identity::Identity::load_or_generate(&cfg_dir)
        .expect("identidad")
        .node_id()
        .to_string();
    let fake_id = "AAAAAAAAAA"; // forma válida, distinta de la real
    assert_ne!(fake_id, real_id);

    // Caché envenenada: el id falso "apunta" al anfitrión real.
    let addr: std::net::SocketAddr = format!("127.0.0.1:{bind_port}").parse().unwrap();
    anyremote_transport::discover::save_resolved(&cfg_dir, fake_id, addr).expect("cache");

    let listen_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let listen = tokio::spawn({
        let cfg_dir = cfg_dir.clone();
        let le = listen_events.clone();
        async move { run_listen(listen_cfg(cfg_dir, bind_port, dead_port, le)).await }
    });
    wait_listening(&listen_events).await;

    let ctl_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let controller = tokio::spawn({
        let cfg_dir = cfg_dir.clone();
        let ce = ctl_events.clone();
        async move { run_connect(connect_cfg(cfg_dir, fake_id, dead_port, ce)).await }
    });

    let err = match controller.await.expect("join ctl") {
        Ok(_) => panic!("el par no es el ID solicitado"),
        Err(e) => e,
    };
    let msg = format!("{err:?}");
    assert!(
        msg.contains("no es el ID solicitado"),
        "rechazo por id distinto: {msg}"
    );

    // Ni TOFU ni pin para el id falso (la verificación va ANTES de guardar).
    assert!(
        !cfg_dir
            .join("trusted")
            .join(format!("{fake_id}.pem"))
            .exists(),
        "no se guarda pin para un id que no correspondió"
    );

    let _ = listen.await.expect("join listener");
    let _ = std::fs::remove_dir_all(&cfg_dir);
}
