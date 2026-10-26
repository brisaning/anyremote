//! Autenticación mutua de sesión: pruebas de identidad ed25519 vinculadas al canal
//! TLS mediante exportación de material de clave (RFC 5705) — D-03, patrones H-RD-02
//! (id_pk + binding) y H-RD-04 (límites de intentos). Ciclo C2 (RF-03/RNF-01).

use tokio::net::TcpStream;
use tokio_rustls::client;
use tokio_rustls::server;

use anyremote_identity::{Identity, NodeId, TrustStore};

use crate::arsp::{proto, Channel};
use crate::TransportError;

/// Etiqueta de exportación de material de clave para el binding de canal.
pub const BINDING_LABEL: &str = "anyremote-auth-v1";

/// Máximo de intentos de autenticación fallidos antes de cortar (patrón H-RD-04).
pub const MAX_AUTH_FAILURES: u8 = 3;

/// Trazado de intentos fallidos de autenticación por conexión.
pub struct AuthGate {
    max: u8,
    failures: u8,
}

impl Default for AuthGate {
    fn default() -> Self {
        Self {
            max: MAX_AUTH_FAILURES,
            failures: 0,
        }
    }
}

impl AuthGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra un fallo; al superar el máximo la conexión se corta.
    pub fn record_failure(&mut self) -> Result<(), TransportError> {
        self.failures += 1;
        if self.failures >= self.max {
            Err(TransportError::Auth(format!(
                "demasiados intentos fallidos de autenticación ({})",
                self.failures
            )))
        } else {
            Err(TransportError::Auth("prueba de identidad inválida".into()))
        }
    }
}

/// Identidad validada del peer autenticado.
#[derive(Debug, Clone, PartialEq)]
pub struct PeerIdentity {
    pub node_id: NodeId,
    pub pubkey: [u8; 32],
    pub trusted: bool,
}

/// Exporta el binding de canal TLS (idéntico en ambos extremos de la sesión TLS).
pub fn binding_of_client(stream: &client::TlsStream<TcpStream>) -> Result<Vec<u8>, TransportError> {
    export(stream.get_ref().1)
}

pub fn binding_of_server(stream: &server::TlsStream<TcpStream>) -> Result<Vec<u8>, TransportError> {
    export(stream.get_ref().1)
}

fn export(conn: &rustls::ConnectionCommon<impl rustls::SideData>) -> Result<Vec<u8>, TransportError> {
    let mut buf = [0u8; 32];
    conn.export_keying_material(&mut buf, BINDING_LABEL.as_bytes(), None)
        .map_err(|e| TransportError::Auth(format!("exportar binding: {e}")))?;
    Ok(buf.to_vec())
}

/// Envía la prueba de identidad local sobre el canal ARSP.
pub async fn send_auth<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    ch: &mut Channel<T>,
    id: &Identity,
    binding: &[u8],
) -> Result<(), TransportError> {
    ch.send(proto::envelope::Payload::Auth(proto::Auth {
        node_id: id.node_id().to_string(),
        pubkey: id.verifying_key_bytes().to_vec(),
        signature: id.sign_auth(binding).to_vec(),
        binding: binding.to_vec(),
    }))
    .await
}

/// Recibe y valida la prueba del peer: firma ed25519 correcta, binding de canal idéntico
/// y node_id derivado coherente con la clave pública (vínculo id↔PK, H-RD-02).
pub async fn recv_validate_auth<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    ch: &mut Channel<T>,
    gate: &mut AuthGate,
    trust: &TrustStore,
    expected_binding: &[u8],
) -> Result<PeerIdentity, TransportError> {
    let env = ch.recv().await?;
    let proof = match env.payload {
        Some(proto::envelope::Payload::Auth(a)) => a,
        _ => {
            gate.record_failure()?;
            unreachable!("record_failure siempre retorna Err");
        }
    };

    let invalid = |gate: &mut AuthGate| gate.record_failure();

    if proof.binding != expected_binding {
        invalid(gate)?;
    }
    let pk: [u8; 32] = match proof.pubkey.as_slice().try_into() {
        Ok(k) => k,
        Err(_) => {
            invalid(gate)?;
            unreachable!();
        }
    };
    let sig: [u8; 64] = match proof.signature.as_slice().try_into() {
        Ok(s) => s,
        Err(_) => {
            invalid(gate)?;
            unreachable!();
        }
    };
    let Ok(derived) = Identity::derive_node_id(&pk) else {
        invalid(gate)?;
        unreachable!();
    };
    let Ok(claimed) = NodeId::parse(&proof.node_id) else {
        invalid(gate)?;
        unreachable!();
    };
    if derived != claimed {
        invalid(gate)?;
    }
    if !Identity::verify_auth(&pk, expected_binding, &sig) {
        invalid(gate)?;
    }

    let trusted = trust.is_trusted(&derived, &pk);
    Ok(PeerIdentity {
        node_id: derived,
        pubkey: pk,
        trusted,
    })
}
