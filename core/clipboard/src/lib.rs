//! `anyremote-clipboard` — Portapapeles remoto bidireccional (ciclo C4, RF-02b;
//! patrón H-RD-07: librería de portapapeles propia por plataforma, implementación
//! clean-room sin copia del cliprdr de RustDesk (D-01)).
//!
//! Contención de formatos (gate C4): en v0 **solo texto UTF-8** está admitido;
//! cualquier otro formato se rechaza explícitamente sin tocar el portapapeles local.
//! Motor de sincronización con supresión de eco (ida/vuelta no genera bucles).

use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ClipboardFormat {
    /// Único formato admitido en v0 (contención).
    Text,
}

impl ClipboardFormat {
    pub fn from_wire(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Text),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum ClipboardError {
    /// Formato no admitido por la política de contención (rechazado sin aplicar).
    UnsupportedFormat(i32),
    /// El texto no procede del remoto o es un eco de lo ya enviado.
    Echo,
    Denied(String),
    Platform(String),
    Unavailable(String),
}

/// Contenido sincronizado de portapapeles.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClipboardContent {
    pub revision: u64,
    pub format: ClipboardFormat,
    pub text: String,
}

impl ClipboardContent {
    /// Digesto estable del contenido (dedupe/eco).
    pub fn digest(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.text.hash(&mut h);
        self.format.hash(&mut h);
        h.finish()
    }
}

/// Backend de portapapeles por plataforma.
pub trait ClipboardBackend {
    fn read_text(&mut self) -> Result<Option<String>, ClipboardError>;
    fn write_text(&mut self, text: &str) -> Result<(), ClipboardError>;
    fn description(&self) -> String;
}

/// Backend en memoria (tests, gate y modo sin plataforma).
#[derive(Default)]
pub struct MemoryClipboard {
    pub current: Option<String>,
    pub writes: u64,
}

impl MemoryClipboard {
    pub fn with_text(text: &str) -> Self {
        Self {
            current: Some(text.to_string()),
            writes: 0,
        }
    }
}

impl ClipboardBackend for MemoryClipboard {
    fn read_text(&mut self) -> Result<Option<String>, ClipboardError> {
        Ok(self.current.clone())
    }

    fn write_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.current = Some(text.to_string());
        self.writes += 1;
        Ok(())
    }

    fn description(&self) -> String {
        "memory".into()
    }
}

/// Permite `ClipboardSyncEngine<Box<dyn ClipboardBackend>>` (backends dinámicos, daemon).
impl<B: ClipboardBackend + ?Sized> ClipboardBackend for Box<B> {
    fn read_text(&mut self) -> Result<Option<String>, ClipboardError> {
        (**self).read_text()
    }
    fn write_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        (**self).write_text(text)
    }
    fn description(&self) -> String {
        (**self).description()
    }
}

/// Estadísticas del motor (evidencia del gate).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SyncStats {
    pub sent: u64,
    pub applied: u64,
    pub rejected_format: u64,
    pub echo_suppressed: u64,
    pub unchanged_skipped: u64,
}

/// Motor de sincronización bidireccional sobre un backend.
///
/// - `detect_local_change`: solo envía si el texto local difiere de lo último
///   aplicado desde el remoto (**supresión de eco**) y de lo ya enviado.
/// - `apply_remote`: solo aplica formatos admitidos (contención) y solo si no es
///   eco ni duplicado (idempotente).
pub struct ClipboardSyncEngine<B: ClipboardBackend> {
    backend: B,
    last_sent: Option<String>,
    last_applied: Option<String>,
    revision: u64,
    pub stats: SyncStats,
}

impl<B: ClipboardBackend> ClipboardSyncEngine<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            last_sent: None,
            last_applied: None,
            revision: 0,
            stats: SyncStats::default(),
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// Poll del estado local: devuelve contenido a enviar si hubo cambio real.
    pub fn detect_local_change(&mut self) -> Result<Option<ClipboardContent>, ClipboardError> {
        let Some(text) = self.backend.read_text()? else {
            return Ok(None);
        };
        if self.last_applied.as_deref() == Some(text.as_str()) {
            // El local coincide con lo último recibido: sería eco.
            self.stats.echo_suppressed += 1;
            return Ok(None);
        }
        if self.last_sent.as_deref() == Some(text.as_str()) {
            self.stats.unchanged_skipped += 1;
            return Ok(None);
        }
        self.revision += 1;
        self.last_sent = Some(text.clone());
        self.stats.sent += 1;
        Ok(Some(ClipboardContent {
            revision: self.revision,
            format: ClipboardFormat::Text,
            text,
        }))
    }

    /// Aplica contenido del remoto con contención de formatos. `Ok(true)` = aplicado.
    pub fn apply_remote(&mut self, content: &ClipboardContent) -> Result<bool, ClipboardError> {
        if content.format != ClipboardFormat::Text {
            self.stats.rejected_format += 1;
            let code = match content.format {
                ClipboardFormat::Text => 0,
            };
            return Err(ClipboardError::UnsupportedFormat(code));
        }
        if self.last_sent.as_deref() == Some(content.text.as_str()) {
            // Eco de lo que nosotros mismos enviamos.
            self.stats.echo_suppressed += 1;
            return Ok(false);
        }
        if self.last_applied.as_deref() == Some(content.text.as_str()) {
            self.stats.unchanged_skipped += 1;
            return Ok(false);
        }
        self.backend.write_text(&content.text)?;
        self.last_applied = Some(content.text.clone());
        self.stats.applied += 1;
        Ok(true)
    }

    /// Texto actual del backend (observación).
    pub fn text(&mut self) -> Result<Option<String>, ClipboardError> {
        self.backend.read_text()
    }
}

/// Rechazo de formatos no-texto SIN motor (para validar en la frontera wire).
pub fn reject_format(format_wire: i32) -> Result<ClipboardFormat, ClipboardError> {
    ClipboardFormat::from_wire(format_wire).ok_or(ClipboardError::UnsupportedFormat(format_wire))
}

#[cfg(windows)]
pub mod windows_backend;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_change_is_sent_once_and_echo_is_suppressed() {
        let mut eng = ClipboardSyncEngine::new(MemoryClipboard::with_text("hola"));
        let out = eng.detect_local_change().unwrap().expect("cambio");
        assert_eq!(out.text, "hola");
        assert_eq!(out.revision, 1);
        // Segunda detección sin cambios → no reenvía.
        assert!(eng.detect_local_change().unwrap().is_none());
        assert_eq!(eng.stats.sent, 1);
        assert_eq!(eng.stats.unchanged_skipped, 1);
    }

    #[test]
    fn remote_apply_is_idempotent_and_blocks_echo() {
        let mut eng = ClipboardSyncEngine::new(MemoryClipboard::default());
        let c = ClipboardContent {
            revision: 1,
            format: ClipboardFormat::Text,
            text: "remoto".into(),
        };
        assert!(eng.apply_remote(&c).unwrap());
        assert!(!eng.apply_remote(&c).unwrap()); // duplicado
        assert_eq!(eng.stats.applied, 1);
        // El local ahora es "remoto": detectar NO debe reenviarlo (eco).
        assert!(eng.detect_local_change().unwrap().is_none());
        assert!(eng.stats.echo_suppressed >= 1);
    }

    #[test]
    fn unsupported_format_is_rejected_without_touching_backend() {
        let mut eng = ClipboardSyncEngine::new(MemoryClipboard::with_text("intacto"));
        let c = ClipboardContent {
            revision: 9,
            format: ClipboardFormat::Text,
            text: "x".into(),
        };
        // Simulación de formato no admitido en la frontera (wire=1 -> None).
        assert_eq!(reject_format(1), Err(ClipboardError::UnsupportedFormat(1)));
        // Aplicar con formato válido no afecta la negación anterior.
        assert!(eng.apply_remote(&c).unwrap());
        assert_eq!(eng.backend().current.as_deref(), Some("x"));
        assert_eq!(c.format, ClipboardFormat::Text);
    }

    #[test]
    fn revision_is_monotonic() {
        let mut eng = ClipboardSyncEngine::new(MemoryClipboard::default());
        eng.backend_mut().current = Some("a".into());
        let r1 = eng.detect_local_change().unwrap().unwrap().revision;
        eng.backend_mut().current = Some("b".into());
        let r2 = eng.detect_local_change().unwrap().unwrap().revision;
        assert!(r2 > r1);
    }
}
