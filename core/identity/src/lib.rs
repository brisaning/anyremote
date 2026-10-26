//! `anyremote-identity` — Identidad ed25519 + derivación de ID Base32 (D-03, patrón H-RD-02).
//! Trazabilidad: ver specs/04-trazabilidad.md. Clean-room D-01: sin copia de código AGPL.

use data_encoding::BASE32_NOPAD;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

/// ID de anyremote: Base32 (RFC 4648 sin padding) del SHA-256 de la clave pública ed25519,
/// truncado a 10 caracteres. Propio (L-01: no replica modelos de identificadores ajenos).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(String);

impl NodeId {
    pub const LEN: usize = 10;
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Validación de forma: 10 caracteres Crockford-estrictos Base32 (A–Z, 2–7).
    pub fn parse(s: &str) -> Result<Self, IdentityError> {
        let ok = s.len() == Self::LEN
            && s.chars().all(|c| matches!(c, 'A'..='Z' | '2'..='7'));
        if ok {
            Ok(Self(s.to_ascii_uppercase()))
        } else {
            Err(IdentityError::MalformedId)
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum IdentityError {
    MalformedId,
}

/// Identidad de un nodo: par ed25519 con derivación de `NodeId`.
pub struct Identity {
    signing: SigningKey,
}

impl Identity {
    pub fn generate() -> Self {
        let mut rng = rand_core::OsRng;
        Self {
            signing: SigningKey::generate(&mut rng),
        }
    }

    pub fn node_id(&self) -> NodeId {
        let digest = Sha256::digest(self.signing.verifying_key().as_bytes());
        let encoded = BASE32_NOPAD.encode(&digest);
        NodeId(encoded[..NodeId::LEN].to_ascii_uppercase())
    }

    pub fn verifying_key_bytes(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.signing.sign(msg).to_bytes()
    }

    /// Verifica una firma contra una clave pública cruda (32 bytes).
    pub fn verify(pk: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> bool {
        let Ok(vk) = VerifyingKey::from_bytes(pk) else {
            return false;
        };
        vk.verify(msg, &Signature::from_bytes(sig)).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_is_deterministic_and_well_formed() {
        let id = Identity::generate().node_id();
        assert_eq!(id.as_str().len(), NodeId::LEN);
        NodeId::parse(id.as_str()).unwrap();
    }

    #[test]
    fn signature_roundtrip_and_rejection() {
        let id = Identity::generate();
        let msg = b"anyremote handshake transcript";
        let sig = id.sign(msg);
        let pk = id.verifying_key_bytes();
        assert!(Identity::verify(&pk, msg, &sig));
        let mut bad = sig;
        bad[0] ^= 0xff;
        assert!(!Identity::verify(&pk, msg, &bad));
    }

    #[test]
    fn node_id_parse_rejects_malformed() {
        assert!(NodeId::parse("short").is_err());
        assert!(NodeId::parse("lowercase01").is_err());
        assert!(NodeId::parse(&"A".repeat(NodeId::LEN)).is_ok());
    }
}
