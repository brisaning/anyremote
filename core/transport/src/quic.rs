//! Transporte UDP/QUIC (quinn) — C-15, diseño en `specs/02` §8.
//!
//! El framing ARSP (C1) se reusa **tal cual**: `StreamPair` adapta los dos
//! streams bidireccionales de una conexión QUIC a un único
//! `AsyncRead + AsyncWrite`, de modo que `Channel<StreamPair>` funciona igual
//! que `Channel<TlsStream<TcpStream>>`. El vídeo de sesión corre por un
//! **stream dedicado** (par #2) para que su pérdida/retransmisión no bloquee
//! control/audio/heartbeats (el problema de campo que motivó C-15).
//!
//! Seguridad (docs/02 §8): TLS 1.3 QUIC = acuerdo de claves; la autenticación
//! real la ancla **C2** — firma ed25519 sobre el material exportado con el
//! MISMO `BINDING_LABEL` que en TCP (RFC 5705 vía
//! `quinn::Connection::export_keying_material`). Un intermediario que termine
//! TLS a cada lado produce bindings distintos → C2 lo rechaza.

use std::net::SocketAddr;
use std::sync::Arc;

use quinn::{ClientConfig, ServerConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

use crate::{install_crypto, TransportError};

/// Re-exports para el daemon (bucle de escucha + intento de conexión).
pub use quinn::{Connection, Endpoint, Incoming};

/// ALPN de la aplicación (v1). Deja puerta a versiones futuras del protocolo.
pub const ALPN: &[u8] = b"anyremote-v1";

/// Ventana de stream = `MAX_FRAME_BYTES` (un frame ARSP completo en vuelo) y
/// ventana de conexión el doble (control + vídeo concurrentes).
const STREAM_WINDOW: u32 = crate::MAX_FRAME_BYTES as u32;
const CONN_WINDOW: u32 = 2 * STREAM_WINDOW;

/// Par de streams bidireccionales de una conexión QUIC como un único
/// `AsyncRead + AsyncWrite` (reuso directo de `Channel<T>`).
pub struct StreamPair {
    send: quinn::SendStream,
    recv: quinn::RecvStream,
}

impl StreamPair {
    pub fn new(send: quinn::SendStream, recv: quinn::RecvStream) -> Self {
        Self { send, recv }
    }
}

/// Prologo de 1 byte que el CLIENTE escribe en cada stream tras abrirlo.
///
/// QUIC solo visibiliza un stream abierto al peer cuando viaja su primer
/// STREAM frame (`open_bi` reserva el ID localmente): sin bytes, el
/// `accept_bi()` del servidor nunca desbloquea aunque el stream exista — y el
/// stream de vídeo puede no escribirse nunca (`--video off`). El servidor lee
/// el prologo ANTES de envolver el par en `Channel` (el framing ARSP arranca
/// limpio) y su valor actúa además de sanidad de protocolo.
pub const STREAM_PROLOGUE: u8 = b'A';

pub async fn write_prologue(send: &mut quinn::SendStream) -> Result<(), TransportError> {
    // `write_all` inherente de quinn (no el trait de tokio).
    send.write_all(&[STREAM_PROLOGUE])
        .await
        .map_err(|e| TransportError::Io(format!("prologo QUIC: {e}")))
}

pub async fn read_prologue(recv: &mut quinn::RecvStream) -> Result<(), TransportError> {
    let mut b = [0u8; 1];
    recv.read_exact(&mut b)
        .await
        .map_err(|e| TransportError::Io(format!("prologo QUIC: {e}")))?;
    if b[0] == STREAM_PROLOGUE {
        Ok(())
    } else {
        Err(TransportError::Wire(format!(
            "prologo QUIC inválido: 0x{:02x}",
            b[0]
        )))
    }
}

// quinn implementa AsyncRead/AsyncWrite de tokio para cada stream por separado;
// aquí se compone el par (Unpin por construcción: los streams de quinn lo son).
impl tokio::io::AsyncRead for StreamPair {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().recv).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for StreamPair {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        <quinn::SendStream as tokio::io::AsyncWrite>::poll_write(
            std::pin::Pin::new(&mut self.get_mut().send),
            cx,
            buf,
        )
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        // Idéntico al AsyncWrite de quinn: la escritura ya es durable en la
        // ventana de emisión local (la retransmisión es asunto de QUIC).
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        // `finish()`: el peer ve EOF ordenado (no un reset).
        <quinn::SendStream as tokio::io::AsyncWrite>::poll_shutdown(
            std::pin::Pin::new(&mut self.get_mut().send),
            cx,
        )
    }
}

/// Transporte tunneado por defecto: sin 0-RTT (no queremos replays de
/// establecimiento), idle 30 s (los heartbeats de app ~7 s lo mantienen vivo;
/// un enlace muerto se detecta igual vía `connection_gone`), ventanas para un
/// frame ARSP completo y margen para control+vídeo concurrentes.
fn transport_config() -> quinn::TransportConfig {
    let mut t = quinn::TransportConfig::default();
    t.max_concurrent_bidi_streams(quinn::VarInt::from_u32(8));
    t.stream_receive_window(quinn::VarInt::from_u32(STREAM_WINDOW));
    t.receive_window(quinn::VarInt::from_u32(CONN_WINDOW));
    t.max_idle_timeout(Some(quinn::IdleTimeout::from(quinn::VarInt::from_u32(
        30_000, // ms
    ))));
    t
}

/// `ServerConfig` QUIC desde el par cert/key del cert-store (el mismo que TCP).
pub fn server_config(
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Result<ServerConfig, TransportError> {
    install_crypto();
    let mut tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map_err(|e| TransportError::Config(format!("quic server tls: {e}")))?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    // QUIC prohúbe cualquier `max_early_data_size` salvo 0 o u32::MAX; 0 = sin 0-RTT.
    tls.max_early_data_size = 0;
    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls)
        .map_err(|e| TransportError::Config(format!("quic server crypto: {e}")))?;
    let mut cfg = ServerConfig::with_crypto(Arc::new(crypto));
    cfg.transport_config(Arc::new(transport_config()));
    Ok(cfg)
}

/// Verificador «acepta cualquier cert del servidor» (solo QUIC, sin pin).
///
/// En UDP no hay bootstrap en claro (no se puede repartir el PEM antes del
/// handshake en la misma conexión) y la autenticación real la ancla **C2**
/// (firma ed25519 + binding de canal exportado, igual que en TCP): un
/// intermediario puede completar el TLS pero NO producir la prueba del par
/// auténtico con el binding que este extremo exportó. Con pin de
/// `cert_store` no se usa este verificador (se verifica contra la raíz).
#[derive(Debug)]
struct AnyServerCert(rustls::crypto::CryptoProvider);

impl rustls::client::danger::ServerCertVerifier for AnyServerCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        // Decorativo: sin cadena de confianza la firma del cert no ancla nada
        // (la ancla es C2). Lo exige la trait; se acepta cualquier firma.
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// `ClientConfig` QUIC: con `root` (pin de `cert_store`) verifica el cert del
/// servidor (rechazo temprano de intermediarios, como TCP); sin pin acepta
/// cualquier cert — la autenticación la ancla C2 (ver `AnyServerCert`).
pub fn client_config(
    root: Option<&CertificateDer<'static>>,
) -> Result<ClientConfig, TransportError> {
    install_crypto();
    let mut tls = match root {
        Some(trusted) => {
            let mut roots = rustls::RootCertStore::empty();
            roots
                .add(trusted.clone())
                .map_err(|e| TransportError::Config(format!("quic root: {e}")))?;
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth()
        }
        None => rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AnyServerCert(
                rustls::crypto::ring::default_provider(),
            )))
            .with_no_client_auth(),
    };
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls)
        .map_err(|e| TransportError::Config(format!("quic client crypto: {e}")))?;
    let mut cfg = ClientConfig::new(Arc::new(crypto));
    cfg.transport_config(Arc::new(transport_config()));
    Ok(cfg)
}

/// Endpoint de servidor QUIC (bind UDP en `addr`, p. ej. el mismo puerto TCP).
pub fn bind_server(
    addr: SocketAddr,
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Result<Endpoint, TransportError> {
    Endpoint::server(server_config(cert, key)?, addr)
        .map_err(|e| TransportError::Io(format!("bind quic {addr}: {e}")))
}

/// Endpoint de cliente (bind efímero; familia según el destino).
pub fn bind_client(target: SocketAddr) -> Result<Endpoint, TransportError> {
    let local: SocketAddr = if target.is_ipv6() {
        "[::]:0".parse().expect("[::]:0 es válido")
    } else {
        "0.0.0.0:0".parse().expect("0.0.0.0:0 es válido")
    };
    Endpoint::client(local).map_err(|e| TransportError::Io(format!("bind cliente quic: {e}")))
}

/// Conecta al destino (el timeout lo pone el caller — el fallback a TCP del
/// daemon trata cualquier fallo como «este par no habla QUIC»).
///
/// El `server_name` es solo SNI: la verificación real es pin (si existe) o C2.
pub async fn connect(
    ep: &Endpoint,
    target: SocketAddr,
) -> Result<quinn::Connection, TransportError> {
    ep.connect(target, "localhost")
        .map_err(|e| TransportError::Io(format!("quic connect: {e}")))?
        .await
        .map_err(|e| TransportError::Io(format!("quic handshake: {e}")))
}

/// Binding de canal C2 sobre QUIC: **misma etiqueta que en TCP** — la prueba
/// ed25519 del peer se valida contra este material en ambos extremos.
pub fn binding_of(conn: &quinn::Connection) -> Result<Vec<u8>, TransportError> {
    let mut buf = [0u8; 32];
    conn.export_keying_material(&mut buf, crate::auth::BINDING_LABEL.as_bytes(), &[])
        .map_err(|e| TransportError::Auth(format!("exportar binding QUIC: {e:?}")))?;
    Ok(buf.to_vec())
}

/// Cert del peer (primer cert de la cadena): para el pin TOFU del cliente
/// (se persiste SOLO tras completar C2, como el bootstrap de TCP).
pub fn peer_cert(conn: &quinn::Connection) -> Option<CertificateDer<'static>> {
    let certs = conn
        .peer_identity()?
        .downcast::<Vec<CertificateDer<'static>>>()
        .ok()?;
    certs.first().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arsp::Channel;

    /// Conexión QUIC de prueba. Los endpoints DEBEN vivir en el caller:
    /// quinn cierra las conexiones al dropear su Endpoint.
    struct Wire {
        _srv: Endpoint,
        _cli: Endpoint,
        conn_s: quinn::Connection,
        conn_c: quinn::Connection,
    }

    async fn pair(with_root: bool) -> Wire {
        let lc = crate::generate_loopback_cert().expect("cert");
        let srv = bind_server(
            "127.0.0.1:0".parse().unwrap(),
            lc.cert.clone(),
            lc.key.clone_key(),
        )
        .expect("server");
        let addr = srv.local_addr().expect("addr");
        let mut cli = bind_client(addr).expect("client");
        let root = with_root.then_some(lc.cert.clone());
        cli.set_default_client_config(client_config(root.as_ref()).expect("cli cfg"));
        // El handshake requiere que AMBOS lados se pisen a la vez (patrón de
        // los propios tests de quinn): `join!` de connect y accept+handshake.
        let (conn_c, conn_s) = tokio::join!(connect(&cli, addr), async {
            let inc = tokio::time::timeout(std::time::Duration::from_secs(5), srv.accept())
                .await
                .expect("accept")
                .expect("incoming");
            inc.await.expect("server handshake")
        });
        Wire {
            _srv: srv,
            _cli: cli,
            conn_s,
            conn_c: conn_c.expect("connect"),
        }
    }

    /// C-15: `Channel<StreamPair>` intercambia frames ARSP cifrados por QUIC
    /// (reuso del framing C1 sin tocar `Channel`).
    #[tokio::test]
    async fn streampair_reuses_arsp_channel_over_quic() {
        let wire = pair(false).await;
        let conn_s = wire.conn_s.clone();
        let conn_c = wire.conn_c.clone();

        let server = tokio::spawn(async move {
            let (s, mut r) = conn_s.accept_bi().await.expect("accept_bi");
            read_prologue(&mut r).await.expect("prologo");
            let mut ch = Channel::new(StreamPair::new(s, r));
            let ping = crate::arsp::proto::envelope::Payload::Bye(crate::arsp::proto::Bye {
                reason: "anyremote::ping".into(),
            });
            let env = ch.recv().await.expect("recv ping");
            assert!(matches!(
                env.payload,
                Some(crate::arsp::proto::envelope::Payload::Bye(ref b))
                    if b.reason == "anyremote::ping"
            ));
            ch.send(ping).await.expect("send pong");
        });

        let (mut s, r) = conn_c.open_bi().await.expect("open_bi");
        write_prologue(&mut s).await.expect("prologo");
        let mut ch = Channel::new(StreamPair::new(s, r));
        ch.send(crate::arsp::proto::envelope::Payload::Bye(
            crate::arsp::proto::Bye {
                reason: "anyremote::ping".into(),
            },
        ))
        .await
        .expect("send ping");
        let env = ch.recv().await.expect("recv pong");
        assert!(matches!(
            env.payload,
            Some(crate::arsp::proto::envelope::Payload::Bye(ref b))
                if b.reason == "anyremote::ping"
        ));
        server.await.expect("server task");
    }

    /// C2 sobre QUIC: ambos extremos exportan el MISMO binding con el
    /// `BINDING_LABEL` de TCP — la prueba ed25519 es intercambiable.
    #[tokio::test]
    async fn binding_is_identical_on_both_ends() {
        let wire = pair(false).await;
        let (conn_s, conn_c) = (wire.conn_s.clone(), wire.conn_c.clone());
        let b_s = binding_of(&conn_s).expect("binding server");
        let b_c = binding_of(&conn_c).expect("binding cliente");
        assert_eq!(b_s, b_c, "el binding de canal debe coincidir");
        assert_eq!(b_s.len(), 32);
    }

    /// Con pin (TOFU previo) el handshake también se completa — y el cert del
    /// peer queda disponible para revalidar/persistir el pin.
    #[tokio::test]
    async fn pinned_handshake_works_and_peer_cert_is_available() {
        let wire = pair(true).await;
        let (conn_s, conn_c) = (wire.conn_s.clone(), wire.conn_c.clone());
        // El CLIENTE ve el cert del servidor → material del pin TOFU (no hay
        // cliente con cert: `with_no_client_auth`, igual que en TCP).
        assert!(
            peer_cert(&conn_c).is_some(),
            "el cliente ve el cert del servidor (pin TOFU)"
        );
        // El binding sigue coincidiendo (la auth C2 no depende del pin).
        assert_eq!(binding_of(&conn_s).unwrap(), binding_of(&conn_c).unwrap());
    }

    /// Reuso directo del transport config: hasta8 bi-streams (control+vídeo).
    #[tokio::test]
    async fn two_streams_open_independently() {
        let wire = pair(false).await;
        let (conn_s, conn_c) = (wire.conn_s.clone(), wire.conn_c.clone());
        let (mut c1s, c1r) = conn_c.open_bi().await.expect("stream 1");
        write_prologue(&mut c1s).await.expect("prologo 1");
        let (mut c2s, c2r) = conn_c.open_bi().await.expect("stream 2");
        write_prologue(&mut c2s).await.expect("prologo 2");
        let (s1s, mut s1r) = conn_s.accept_bi().await.expect("accept 1");
        read_prologue(&mut s1r).await.expect("leer prologo 1");
        let (s2s, mut s2r) = conn_s.accept_bi().await.expect("accept 2");
        read_prologue(&mut s2r).await.expect("leer prologo 2");
        let c1 = (c1s, c1r);
        let s1 = (s1s, s1r);
        let c2 = (c2s, c2r);
        let s2 = (s2s, s2r);
        // El orden de apertura es determinista (IDs de stream crecientes).
        assert_eq!(c1.0.id().index(), s1.0.id().index());
        assert_eq!(c2.0.id().index(), s2.0.id().index());
    }
}
