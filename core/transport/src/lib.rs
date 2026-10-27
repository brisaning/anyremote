//! `anyremote-transport` — Canal TCP+TLS 1.3 (rustls), framing ARSP y trait Rendezvous (D-03/D-04/D-08,
//! patrones H-RD-01/H-RD-04). Trazabilidad: specs/04-trazabilidad.md. Clean-room D-01.

use std::sync::Once;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub mod arsp;
pub mod auth;

/// Máximo de un frame ARSP (16 MiB) — cota de recurso del protocolo.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Instala el proveedor crypto `ring` de rustls una sola vez por proceso.
pub fn install_crypto() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Certificado auto-firmado del nodo servidor (S0: "handshake simulado" — el DER del cert
/// se distribuye fuera de banda; C2 lo reemplaza por pinning ed25519 vía `identity`).
pub struct LoopbackCert {
    pub cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
}

pub fn generate_loopback_cert() -> Result<LoopbackCert, TransportError> {
    install_crypto();
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .map_err(|e| TransportError::Cert(e.to_string()))?;
    let cert = CertificateDer::from(certified.cert.der().to_vec());
    let key = PrivateKeyDer::Pkcs8(certified.key_pair.serialize_der().into());
    Ok(LoopbackCert { cert, key })
}

pub fn server_config(cert: &LoopbackCert) -> Result<ServerConfig, TransportError> {
    ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert.cert.clone()], cert.key.clone_key())
        .map_err(|e| TransportError::Config(e.to_string()))
}

pub fn client_config(trusted: &CertificateDer<'static>) -> Result<ClientConfig, TransportError> {
    let mut roots = RootCertStore::empty();
    roots
        .add(trusted.clone())
        .map_err(|e| TransportError::Config(e.to_string()))?;
    Ok(ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

/// Re-exporta pki-types para consumidores (daemon).
pub use rustls::pki_types;

/// ServerConfig desde cert/key ya cargados (cert-store persistido).
pub fn server_config_from(
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Result<ServerConfig, TransportError> {
    ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map_err(|e| TransportError::Config(e.to_string()))
}

/// Certificado auto-firmado persistido en el directorio de configuración.
///
/// Confianza v0.1 (localhost): `server_cert.pem`/`server_key.pem` compartidos por
/// ambos roles en la misma máquina; el cliente lo usa como root store (validación
/// real). Cross-machine: distribuir `server_cert.pem` (limitación documentada).
pub mod cert_store {
    use std::path::Path;

    use rustls::pki_types::{CertificateDer, PrivateKeyDer};

    use super::TransportError;

    const CERT_FILE: &str = "server_cert.pem";
    const KEY_FILE: &str = "server_key.pem";

    pub fn ensure(
        dir: &Path,
    ) -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>), TransportError> {
        std::fs::create_dir_all(dir).map_err(|e| TransportError::Io(e.to_string()))?;
        let cp = dir.join(CERT_FILE);
        let kp = dir.join(KEY_FILE);
        if cp.exists() && kp.exists() {
            let cert_pem =
                std::fs::read_to_string(&cp).map_err(|e| TransportError::Io(e.to_string()))?;
            let key_pem =
                std::fs::read_to_string(&kp).map_err(|e| TransportError::Io(e.to_string()))?;
            let cert = cert_from_pem(&cert_pem)?;
            let key = rustls_pemfile::private_key(&mut key_pem.as_bytes())
                .map_err(|e| TransportError::Config(format!("PEM key: {e}")))?
                .ok_or_else(|| TransportError::Config("server_key.pem sin clave privada".into()))?;
            return Ok((cert, key));
        }

        let generated = rcgen::generate_simple_self_signed(vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ])
        .map_err(|e| TransportError::Cert(e.to_string()))?;
        let cert = CertificateDer::from(generated.cert.der().to_vec());
        let key = PrivateKeyDer::Pkcs8(generated.key_pair.serialize_der().into());
        std::fs::write(&cp, generated.cert.pem()).map_err(|e| TransportError::Io(e.to_string()))?;
        std::fs::write(&kp, generated.key_pair.serialize_pem())
            .map_err(|e| TransportError::Io(e.to_string()))?;
        Ok((cert, key))
    }

    /// PEM del cert del servidor si existe.
    pub fn read_pem(dir: &Path) -> Result<Option<String>, TransportError> {
        let cp = dir.join(CERT_FILE);
        if !cp.exists() {
            return Ok(None);
        }
        std::fs::read_to_string(&cp)
            .map(Some)
            .map_err(|e| TransportError::Io(e.to_string()))
    }

    /// PEM → certificado DER.
    pub fn cert_from_pem(pem: &str) -> Result<CertificateDer<'static>, TransportError> {
        rustls_pemfile::certs(&mut pem.as_bytes())
            .next()
            .ok_or_else(|| TransportError::Config("PEM sin certificados".into()))?
            .map_err(|e| TransportError::Config(format!("PEM cert: {e}")))
    }

    /// Huella SHA-256 (hex) de un certificado — para pinning.
    pub fn cert_fingerprint(cert: &CertificateDer<'static>) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(cert.as_ref());
        data_encoding::HEXLOWER.encode(&h.finalize())
    }

    /// Guarda el PEM recibido como raíz local (pin) — solo tras auth mutua exitosa.
    pub fn save_pem(dir: &Path, pem: &str) -> Result<(), TransportError> {
        std::fs::create_dir_all(dir).map_err(|e| TransportError::Io(e.to_string()))?;
        std::fs::write(dir.join(CERT_FILE), pem).map_err(|e| TransportError::Io(e.to_string()))
    }

    fn sanitize_target(target: &str) -> String {
        target
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect()
    }

    /// Ruta del pin de un destino concreto: `trusted/<target_sanitizado>.pem`.
    /// Un pin por destino evita que el cert de TU propio listener contaminé
    /// la confianza hacia otros servidores (fallo observado en prueba cross-device).
    pub fn trusted_path(dir: &Path, target: &str) -> std::path::PathBuf {
        dir.join("trusted")
            .join(format!("{}.pem", sanitize_target(target)))
    }

    pub fn load_trusted(dir: &Path, target: &str) -> Result<Option<String>, TransportError> {
        let p = trusted_path(dir, target);
        if !p.exists() {
            return Ok(None);
        }
        std::fs::read_to_string(p)
            .map(Some)
            .map_err(|e| TransportError::Io(e.to_string()))
    }

    pub fn save_trusted(dir: &Path, target: &str, pem: &str) -> Result<(), TransportError> {
        let p = trusted_path(dir, target);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| TransportError::Io(e.to_string()))?;
        }
        std::fs::write(p, pem).map_err(|e| TransportError::Io(e.to_string()))
    }

    const BOOTSTRAP_MAGIC: &str = "ANYREMOTE-CERT-V1";

    /// El servidor envía su PEM de raíz en claro ANTES del TLS (dato público;
    /// la autenticación real la ancla la capa de aplicación con binding de canal).
    pub async fn write_bootstrap(
        tcp: &mut tokio::net::TcpStream,
        pem: &str,
    ) -> Result<(), TransportError> {
        use tokio::io::AsyncWriteExt;
        let bytes = pem.as_bytes();
        let frame = format!("{BOOTSTRAP_MAGIC}\n{}\n", bytes.len());
        tcp.write_all(frame.as_bytes())
            .await
            .map_err(|e| TransportError::Io(e.to_string()))?;
        tcp.write_all(bytes)
            .await
            .map_err(|e| TransportError::Io(e.to_string()))?;
        tcp.write_all(b"\n")
            .await
            .map_err(|e| TransportError::Io(e.to_string()))?;
        tcp.flush()
            .await
            .map_err(|e| TransportError::Io(e.to_string()))?;
        Ok(())
    }

    /// Cliente: lee el bootstrap con tope de 2 s → (PEM crudo, cert).
    /// `None` = el par no lo envió (compatibilidad).
    pub async fn read_bootstrap(
        tcp: &mut tokio::net::TcpStream,
    ) -> Result<Option<(String, CertificateDer<'static>)>, TransportError> {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let mut reader = BufReader::new(&mut *tcp);
            let mut magic = String::new();
            reader
                .read_line(&mut magic)
                .await
                .map_err(|e| TransportError::Io(e.to_string()))?;
            if magic.trim() != BOOTSTRAP_MAGIC {
                return Err(TransportError::Config(format!(
                    "bootstrap inesperado: {:?}",
                    magic.trim()
                )));
            }
            let mut len_line = String::new();
            reader
                .read_line(&mut len_line)
                .await
                .map_err(|e| TransportError::Io(e.to_string()))?;
            let len: usize = len_line
                .trim()
                .parse()
                .map_err(|_| TransportError::Config("longitud de bootstrap inválida".into()))?;
            if len > 64 * 1024 {
                return Err(TransportError::Config("bootstrap excesivo".into()));
            }
            let mut pem = vec![0u8; len];
            reader
                .read_exact(&mut pem)
                .await
                .map_err(|e| TransportError::Io(e.to_string()))?;
            let mut nl = [0u8; 1];
            let _ = reader.read_exact(&mut nl).await;
            let pem = String::from_utf8(pem)
                .map_err(|_| TransportError::Config("bootstrap no es UTF-8".into()))?;
            let cert = cert_from_pem(&pem)?;
            Ok((pem, cert))
        })
        .await;
        match result {
            Ok(inner) => inner.map(Some),
            Err(_elapsed) => Ok(None), // sin bootstrap (compat): el caller decide
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum TransportError {
    Cert(String),
    Config(String),
    Io(String),
    FrameTooLarge(usize),
    Auth(String),
    /// Mensaje wire malformado o con campos inválidos.
    Wire(String),
    Eof,
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Escribe un frame: longitud u32 big-endian + payload.
pub async fn write_frame<W: AsyncWrite + Unpin>(
    w: &mut W,
    payload: &[u8],
) -> Result<(), TransportError> {
    if payload.len() > MAX_FRAME_BYTES {
        return Err(TransportError::FrameTooLarge(payload.len()));
    }
    let len = (payload.len() as u32).to_be_bytes();
    w.write_all(&len).await.map_err(io_err)?;
    w.write_all(payload).await.map_err(io_err)?;
    w.flush().await.map_err(io_err)?;
    Ok(())
}

/// Lee un frame respetando la cota `MAX_FRAME_BYTES`.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Vec<u8>, TransportError> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            TransportError::Eof
        } else {
            io_err(e)
        }
    })?;
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_FRAME_BYTES {
        return Err(TransportError::FrameTooLarge(n));
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf).await.map_err(io_err)?;
    Ok(buf)
}

fn io_err(e: std::io::Error) -> TransportError {
    TransportError::Io(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyremote_session::{SessionEvent, SessionState};
    use tokio::net::{TcpListener, TcpStream};
    use tokio_rustls::{TlsAcceptor, TlsConnector};

    /// Gate de ciclo S0: dos nodos intercambian bytes cifrados en loopback
    /// y la sesión avanza Handshaking → AwaitingPermission → Active → Closed.
    #[tokio::test]
    async fn e2e_session_loopback_encrypted_exchange() {
        let loop_cert = generate_loopback_cert().expect("cert");
        let srv_cfg = server_config(&loop_cert).expect("srv cfg");
        let cli_cfg = client_config(&loop_cert.cert).expect("cli cfg");

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let acceptor = TlsAcceptor::from(std::sync::Arc::new(srv_cfg));
            let (tcp, _) = listener.accept().await.expect("accept");
            let mut tls = acceptor.accept(tcp).await.expect("tls accept");
            let ping = read_frame(&mut tls).await.expect("read ping");
            assert_eq!(ping, b"anyremote::ping");
            write_frame(&mut tls, b"anyremote::pong")
                .await
                .expect("write pong");
        });

        let connector = TlsConnector::from(std::sync::Arc::new(cli_cfg));
        let tcp = TcpStream::connect(addr).await.expect("connect");
        let server_name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
        let mut tls = connector
            .connect(server_name, tcp)
            .await
            .expect("tls connect");

        let mut state = SessionState::Handshaking;
        state = state.transition(SessionEvent::HandshakeOk).unwrap();
        assert_eq!(state, SessionState::AwaitingPermission);
        state = state.transition(SessionEvent::PermissionGranted).unwrap();
        assert_eq!(state, SessionState::Active);

        write_frame(&mut tls, b"anyremote::ping")
            .await
            .expect("write ping");
        let pong = read_frame(&mut tls).await.expect("read pong");
        assert_eq!(pong, b"anyremote::pong");

        state = state.transition(SessionEvent::StartClose).unwrap();
        assert_eq!(
            state.transition(SessionEvent::Closed).unwrap(),
            SessionState::Closed
        );
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn frame_size_limit_is_enforced() {
        let big = vec![0u8; MAX_FRAME_BYTES + 1];
        let mut out: Vec<u8> = Vec::new();
        assert_eq!(
            write_frame(&mut out, &big).await,
            Err(TransportError::FrameTooLarge(MAX_FRAME_BYTES + 1))
        );
    }
}
