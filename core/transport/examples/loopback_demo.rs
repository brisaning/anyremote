//! Demo del tronco S0: dos nodos (servidor y cliente) intercambian bytes cifrados
//! en loopback sobre TLS 1.3 con identidades ed25519, y la sesión recorre su
//! máquina de estados completa. Ejecutar: `cargo run -p anyremote-transport --example loopback_demo`
use std::sync::Arc;

use anyremote_identity::Identity;
use anyremote_session::{PermissionSet, SessionEvent, SessionState};
use anyremote_transport::{client_config, generate_loopback_cert, read_frame, server_config, write_frame};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

#[tokio::main]
async fn main() {
    println!("=== anyremote S0 — demo de tronco (canal cifrado loopback) ===");

    let server_id = Identity::generate();
    let client_id = Identity::generate();
    println!("[identity] servidor id = {}", server_id.node_id().as_str());
    println!("[identity] cliente id = {}", client_id.node_id().as_str());

    let cert = generate_loopback_cert().expect("cert TLS");
    let srv_cfg = server_config(&cert).expect("config servidor");
    let cli_cfg = client_config(&cert.cert).expect("config cliente");

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();
    println!("[transport] servidor TLS en {addr}");

    let server = tokio::spawn(async move {
        let acceptor = TlsAcceptor::from(Arc::new(srv_cfg));
        let (tcp, peer) = listener.accept().await.expect("accept");
        println!("[transport] conexión entrante de {peer} — completando handshake TLS…");
        let mut tls = acceptor.accept(tcp).await.expect("aceptar TLS");
        let ping = read_frame(&mut tls).await.expect("leer ping");
        println!("[servidor] recibió frame cifrado: {:?} ({} bytes)", String::from_utf8_lossy(&ping), ping.len());
        write_frame(&mut tls, b"anyremote::pong::concedido").await.expect("escribir pong");
        println!("[servidor] envió concesión de permisos");
    });

    let mut state = SessionState::Handshaking;
    let connector = TlsConnector::from(Arc::new(cli_cfg));
    let tcp = TcpStream::connect(addr).await.expect("conectar");
    let sni = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let mut tls = connector.connect(sni, tcp).await.expect("cliente TLS");

    state = state.transition(SessionEvent::HandshakeOk).unwrap();
    println!("[sesión] Handshaking -> AwaitingPermission");
    let perms = PermissionSet::none();
    println!("[permisos] denegados por defecto: keyboard={} mouse={} clipboard={}", perms.keyboard, perms.mouse, perms.clipboard);

    write_frame(&mut tls, b"anyremote::ping").await.expect("enviar ping");
    println!("[cliente] envió ping cifrado");
    let pong = read_frame(&mut tls).await.expect("leer pong");
    println!("[cliente] respuesta cifrada: {:?}", String::from_utf8_lossy(&pong));

    state = state.transition(SessionEvent::PermissionGranted).unwrap();
    println!("[sesión] AwaitingPermission -> Active");
    state = state.transition(SessionEvent::StartClose).unwrap();
    state = state.transition(SessionEvent::Closed).unwrap();
    println!("[sesión] Active -> Closing -> Closed");
    server.await.expect("tarea servidor");

    println!("=== demo completado: canal cifrado E2E operativo (gate S0) ===");
}
