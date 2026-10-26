//! `anyremote-identity` — Identidad ed25519 + derivación de ID Base32 (D-03, patrón H-RD-02).
//! Trazabilidad: ver specs/04-trazabilidad.md. Clean-room D-01: sin copia de código AGPL.

use data_encoding::BASE32_NOPAD;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

/// ID de anyremote: Base32 (RFC 4648 sin padding) del SHA-256 de la clave pública ed25519,
/// truncado a 10 caracteres. Propio (L-01: no replica modelos de identificadores ajenos).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct NodeId(String);

impl NodeId {
    pub const LEN: usize = 10;
    pub fn as_str(&self) -> &str {
        &self.0
    }    /// Validación de forma: 10 caracteres Crockford-estrictos Base32 (A–Z, 2–7).
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

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
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

/// Dominio de separación para las firmas de autenticación de canal (D-03).
pub const AUTH_DOMAIN: &[u8] = b"anyremote-auth-v1";

fn transcript(binding: &[u8]) -> Vec<u8> {
    let mut t = Vec::from(AUTH_DOMAIN);
    t.extend_from_slice(&(binding.len() as u64).to_be_bytes());
    t.extend_from_slice(binding);
    t
}

fn derive_node_id_from_pk_bytes(pk: &[u8; 32]) -> NodeId {
    let digest = Sha256::digest(pk);
    let encoded = BASE32_NOPAD.encode(&digest);
    NodeId(encoded[..NodeId::LEN].to_ascii_uppercase())
}

impl Identity {
    pub fn generate() -> Self {
        let mut rng = rand_core::OsRng;
        Self {
            signing: SigningKey::generate(&mut rng),
        }
    }

    pub fn node_id(&self) -> NodeId {
        derive_node_id_from_pk_bytes(&self.verifying_key_bytes())
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

    /// Firma el binding de canal TLS (RF-03/D-03, patrón H-RD-02).
    pub fn sign_auth(&self, binding: &[u8]) -> [u8; 64] {
        self.sign(&transcript(binding))
    }

    /// Verifica una prueba de autenticación completa.
    pub fn verify_auth(pk: &[u8; 32], binding: &[u8], sig: &[u8; 64]) -> bool {
        Identity::verify(pk, &transcript(binding), sig)
    }

    /// Deriva el NodeId a partir de una clave pública cruda (vínculo id ↔ PK, H-RD-02).
    pub fn derive_node_id(pk: &[u8; 32]) -> Result<NodeId, IdentityError> {
        VerifyingKey::from_bytes(pk).map_err(|_| IdentityError::MalformedId)?;
        Ok(derive_node_id_from_pk_bytes(pk))
    }
}

/// Dispositivos de confianza locales (patrón OPTION_ENABLE_TRUSTED_DEVICES, H-RD-02).
#[derive(Default)]
pub struct TrustStore {
    entries: std::collections::BTreeMap<NodeId, [u8; 32]>,
}

impl TrustStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn trust(&mut self, id: NodeId, pk: [u8; 32]) {
        self.entries.insert(id, pk);
    }

    pub fn remove(&mut self, id: &NodeId) -> bool {
        self.entries.remove(id).is_some()
    }

    /// Solo es confiable si el id y la pk coinciden con el registro.
    pub fn is_trusted(&self, id: &NodeId, pk: &[u8; 32]) -> bool {
        self.entries.get(id).is_some_and(|k| k == pk)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
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

    #[test]
    fn auth_proof_binds_channel_and_rejects_tampering() {
        let id = Identity::generate();
        let binding = b"tls-exporter-material-32-bytes!";
        let sig = id.sign_auth(binding);
        let pk = id.verifying_key_bytes();

        assert!(Identity::verify_auth(&pk, binding, &sig));
        // Otro binding (otro canal) rechaza la misma firma.
        assert!(!Identity::verify_auth(&pk, b" otro binding", &sig));
        // Firma alterada rechazada.
        let mut bad = sig;
        bad[10] ^= 0x01;
        assert!(!Identity::verify_auth(&pk, binding, &bad));
        // Clave alterada rechazada.
        let mut bad_pk = pk;
        bad_pk[0] ^= 0x01;
        assert!(!Identity::verify_auth(&bad_pk, binding, &sig));
    }

    #[test]
    fn derived_node_id_matches_signer() {
        for _ in 0..8 {
            let id = Identity::generate();
            assert_eq!(
                Identity::derive_node_id(&id.verifying_key_bytes()).unwrap(),
                id.node_id()
            );
        }
        // Existen codificaciones de32 bytes que no son puntos ed25519 válidos:
        // alguna x en 0..=255 no tiene y en la curva y debe rechazarse.
        let mut rejected = false;
        for b in 0..=255u8 {
            let mut k = [0u8; 32];
            k[0] = b;
            if Identity::derive_node_id(&k).is_err() {
                rejected = true;
                break;
            }
        }
        assert!(rejected, "debe existir alguna codificación inválida");
    }

    #[test]
    fn trust_store_requires_matching_id_and_pk() {
        let a = Identity::generate();
        let b = Identity::generate();
        let mut store = TrustStore::new();
        store.trust(a.node_id(), a.verifying_key_bytes());

        assert!(store.is_trusted(&a.node_id(), &a.verifying_key_bytes()));
        assert!(!store.is_trusted(&b.node_id(), &b.verifying_key_bytes()));
        // Misma id con pk ajena no es confiable.
        assert!(!store.is_trusted(&a.node_id(), &b.verifying_key_bytes()));
        assert!(store.remove(&a.node_id()));
        assert!(store.is_empty());
    }
}
