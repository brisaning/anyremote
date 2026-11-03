//! Gate C-17 — rendezvous + relay (WAN, `specs/02` §10):
//!
//! 1. `resolve_via_rendezvous_and_connect_direct` — el anfitrión se registra
//!    en `anyremote-rdv` (servidor en loopback) y el controlador, tras fallar
//!    el broadcast LAN, resuelve por el servidor (`resolve_ok source:rdv`) y
//!    conecta **directo** a la dirección observada: sesión completa.
//! 2. `direct_failure_falls_back_to_relay` — el puerto registrado está CERRADO
//!    (`rdv_port` de test) → lo directo rechaza → evento `relay_fallback` →
//!    **sesión a través de la tubería de relay** (TLS E2E sobre el pipeline).
//! 3. `rdv_config_via_file_and_offline_error` — el servidor se configura por
//!    el fichero `<config>/rdv` (sin bandera) y un id sin registrar produce
//!    `rdv_error` + el error final accionable.

use std::sync::{Arc, Mutex};

use anyremote_daemon::{run_connect, run_listen, ClipboardMode, DaemonConfig};

fn mk(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("anyremote-rdv-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    d
}

/// Puerto no efímero reservado (patrón de `daemon_quic_e2e`).
async fn free_port() -> u16 {
    use std::sync::atomic::{AtomicU16, Ordering};
    static NEXT: AtomicU16 = AtomicU16::new(0);
    let off = NEXT.fetch_add(4, Ordering::SeqCst);
    // Banda 46000..47511 garantizada: NUNCA alcanza el rango efímero de
    // Windows (>=49152) — con `47_000 + %4_000` ciertos pids del runner caían
    // en efímeros ocupados y los tests panicaban al empezar (CI de main).
    let base = 46_000u16
        .wrapping_add((std::process::id() as u16).wrapping_mul(7) % 1_500)
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

/// Servidor de encuentro en loopback (en proceso).
async fn start_rdv() -> std::net::SocketAddr {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("rdv bind");
    let addr = l.local_addr().expect("rdv addr");
    tokio::spawn(anyremote_rendezvous::serve(
        l,
        anyremote_rendezvous::Limits::default(),
    ));
    addr
}

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
            "listener sin listening: {:?}",
            events.lock().unwrap()
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

fn listen_cfg(
    dir: std::path::PathBuf,
    bind_port: u16,
    disc_port: u16,
    rdv: Option<&str>,
    rdv_port: Option<u16>,
    events: Arc<Mutex<Vec<String>>>,
) -> DaemonConfig {
    let mut c = DaemonConfig::new(dir);
    c.bind = format!("127.0.0.1:{bind_port}");
    c.clipboard = ClipboardMode::Off;
    c.discover = format!("127.0.0.1:{disc_port}");
    c.rdv = rdv.map(|s| s.to_string());
    c.rdv_port = rdv_port;
    c.max_secs = 8;
    c.on_event = Some(Box::new(move |l: String| events.lock().unwrap().push(l)));
    c
}

fn connect_cfg(
    dir: std::path::PathBuf,
    target: &str,
    disc_port: u16,
    rdv: Option<&str>,
    events: Arc<Mutex<Vec<String>>>,
) -> DaemonConfig {
    let mut c = DaemonConfig::new(dir);
    c.target = Some(target.to_string());
    c.clipboard = ClipboardMode::Off;
    c.discover = format!("127.0.0.1:{disc_port}");
    c.rdv = rdv.map(|s| s.to_string());
    c.max_secs = 4;
    c.on_event = Some(Box::new(move |l: String| events.lock().unwrap().push(l)));
    c
}

/// 1) Resolución por rendezvous + conexión DIRECTA (la IP observada + el
///    puerto de escucha reales) → sesión completa.
#[tokio::test]
async fn resolve_via_rendezvous_and_connect_direct() {
    let rdv = start_rdv().await;
    let cfg_dir = mk("direct");
    let bind_port = free_port().await;
    let disc_c = free_port().await;
    let disc_h = free_port().await;
    let node_id = anyremote_identity::Identity::load_or_generate(&cfg_dir)
        .expect("identidad")
        .node_id()
        .to_string();

    let listen_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let listen = tokio::spawn({
        let cfg_dir = cfg_dir.clone();
        let le = listen_events.clone();
        let rdv = rdv.to_string();
        async move { run_listen(listen_cfg(cfg_dir, bind_port, disc_h, Some(&rdv), None, le)).await }
    });
    wait_listening(&listen_events).await;
    // el anfitrión se registró en el rendezvous (evento de arranque)
    assert!(
        listen_events
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains("\"event\":\"rdv_bound\"")),
        "rdv_bound: {:?}",
        listen_events.lock().unwrap()
    );

    let ctl_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let controller = tokio::spawn({
        let cfg_dir = cfg_dir.clone();
        let ce = ctl_events.clone();
        let rdv = rdv.to_string();
        let id = node_id.clone();
        async move { run_connect(connect_cfg(cfg_dir, &id, disc_c, Some(&rdv), ce)).await }
    });

    controller
        .await
        .expect("join ctl")
        .expect("sesion por rdv completa");
    let _srv = listen
        .await
        .expect("join listener")
        .expect("listener completo");

    let ctl_text = ctl_events.lock().unwrap().join("\n");
    let srv_text = listen_events.lock().unwrap().join("\n");
    // Resuelto por el servidor (el broadcast LAN está en un puerto muerto).
    assert!(
        ctl_text.contains("\"event\":\"resolve_ok\"") && ctl_text.contains("\"source\":\"rdv\""),
        "resolucion via rdv:\n{ctl_text}"
    );
    // Conexión directa (sin relay): NO debe aparecer relay_fallback.
    assert!(
        !ctl_text.contains("relay_fallback"),
        "sin fallback a relay:\n{ctl_text}"
    );
    assert!(
        ctl_text.contains("\"event\":\"session_active\"")
            && srv_text.contains("\"event\":\"session_active\""),
        "sesion completa:\n{ctl_text}\n{srv_text}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// 2) El puerto registrado está cerrado → lo directo rechaza → RELAY → la
///    sesión corre por la tubería (TLS E2E sobre el pipeline del servidor).
#[tokio::test]
async fn direct_failure_falls_back_to_relay() {
    let rdv = start_rdv().await;
    let cfg_dir = mk("relay");
    let bind_port = free_port().await;
    let closed_port = free_port().await; // registrado pero SIN listener
    let disc_c = free_port().await;
    let disc_h = free_port().await;
    let node_id = anyremote_identity::Identity::load_or_generate(&cfg_dir)
        .expect("identidad")
        .node_id()
        .to_string();

    let listen_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let listen = tokio::spawn({
        let cfg_dir = cfg_dir.clone();
        let le = listen_events.clone();
        let rdv = rdv.to_string();
        async move {
            run_listen(listen_cfg(
                cfg_dir,
                bind_port,
                disc_h,
                Some(&rdv),
                Some(closed_port), // ← el endpoint DIRECTO apunta a un puerto cerrado
                le,
            ))
            .await
        }
    });
    wait_listening(&listen_events).await;

    let ctl_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let controller = tokio::spawn({
        let cfg_dir = cfg_dir.clone();
        let ce = ctl_events.clone();
        let rdv = rdv.to_string();
        let id = node_id.clone();
        async move { run_connect(connect_cfg(cfg_dir, &id, disc_c, Some(&rdv), ce)).await }
    });

    controller
        .await
        .expect("join ctl")
        .expect("sesion por relay completa");
    let _srv = listen
        .await
        .expect("join listener")
        .expect("listener completo");

    let ctl_text = ctl_events.lock().unwrap().join("\n");
    assert!(
        ctl_text.contains("\"event\":\"relay_fallback\""),
        "evento relay_fallback (lo directo fallo y se uso el pipeline):\n{ctl_text}"
    );
    assert!(
        ctl_text.contains("\"event\":\"session_active\""),
        "sesion completa a traves del relay:\n{ctl_text}"
    );
    assert!(
        ctl_text.contains("\"source\":\"rdv\""),
        "resuelto por el servidor:\n{ctl_text}"
    );

    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// 3) Configuración por FICHERO `<config>/rdv` (sin bandera) y cadena de
///    error cuando el id no está registrado (evento `rdv_error`).
#[tokio::test]
async fn rdv_config_via_file_and_offline_error() {
    let rdv = start_rdv().await;
    let cfg_dir = mk("file");
    // El fichero lo leen AMBOS roles (misma carpeta que los e2e existentes).
    std::fs::write(cfg_dir.join("rdv"), format!("{rdv}\n")).expect("fichero rdv");
    let bind_port = free_port().await;
    let disc_c = free_port().await;
    let disc_h = free_port().await;
    let node_id = anyremote_identity::Identity::load_or_generate(&cfg_dir)
        .expect("identidad")
        .node_id()
        .to_string();
    let offline_id = "AAAAAAAAAA"; // id con forma válida y sin registrar
    assert_ne!(offline_id, node_id);

    let listen_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let listen = tokio::spawn({
        let cfg_dir = cfg_dir.clone();
        let le = listen_events.clone();
        async move { run_listen(listen_cfg(cfg_dir, bind_port, disc_h, None, None, le)).await }
    });
    wait_listening(&listen_events).await;

    let ctl_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let controller = tokio::spawn({
        let cfg_dir = cfg_dir.clone();
        let ce = ctl_events.clone();
        async move { run_connect(connect_cfg(cfg_dir, offline_id, disc_c, None, ce)).await }
    });

    let err = match controller.await.expect("join ctl") {
        Ok(_) => panic!("el id no registrado debe fallar"),
        Err(e) => e,
    };
    let msg = format!("{err:?}");
    assert!(msg.contains("no se pudo resolver"), "mensaje: {msg}");

    let ctl_text = ctl_events.lock().unwrap().join("\n");
    // El servidor respondió (por el FICHERO, sin bandera) «offline»…
    assert!(
        ctl_text.contains("\"event\":\"rdv_error\"") && ctl_text.contains("offline"),
        "evento rdv_error con el codigo del servidor:\n{ctl_text}"
    );

    let _srv = listen.await.expect("join listener");
    let _ = std::fs::remove_dir_all(&cfg_dir);
}
