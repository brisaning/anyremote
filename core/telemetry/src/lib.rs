//! `anyremote-telemetry` — Registro de sesiones y diagnóstico (ciclo C7, RF-05).
//!
//! **Gap de RF-05** (matriz de trazabilidad: sin análogo en RustDesk → diseño propio, D-10):
//! log de auditoría **JSONL** por sesión (spec §9: evento con tipo, referencia y detalle)
//! + `SessionReport` de diagnóstico enlace→cierre.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Tipos de evento de auditoría (modelo spec §9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventKind {
    SessionStarted,
    SessionEstablished,
    PermissionGranted,
    PermissionDenied,
    HeartbeatSummary,
    FileTransfer,
    ClipboardSync,
    ProfileApplied,
    /// RF-02a: evento de entrada inyectado en el controlado (bajo permiso D-07).
    InputInjected,
    /// RF-02a: evento de entrada rechazado (sin `--allow-input`, sin permiso o no soportado).
    InputDenied,
    Error,
    Disconnected,
}

/// Evento de auditoría: una línea JSONL por evento.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub ts_unix_ms: u64,
    pub kind: AuditEventKind,
    pub session_id: String,
    #[serde(default)]
    pub detail: serde_json::Value,
}

#[derive(Debug, PartialEq)]
pub enum TelemetryError {
    Io(String),
    Json(String),
    MalformedLine(String),
}

impl std::fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Log de auditoría JSONL (un archivo por apertura).
pub struct SessionLog {
    file: File,
    path: PathBuf,
}

impl SessionLog {
    /// Abre/crea `dir/audit-<unix_ms>.jsonl`.
    pub fn open(dir: &Path) -> Result<Self, TelemetryError> {
        std::fs::create_dir_all(dir).map_err(|e| TelemetryError::Io(e.to_string()))?;
        let path = dir.join(format!(
            "audit-{}-{}.jsonl",
            now_unix_ms(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)
            .map_err(|e| TelemetryError::Io(e.to_string()))?;
        Ok(Self { file, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Registra un evento (inmediato en disco — flush por evento: la traza manda sobre el rendimiento aquí).
    pub fn record(
        &mut self,
        kind: AuditEventKind,
        session_id: &str,
        detail: serde_json::Value,
    ) -> Result<AuditEvent, TelemetryError> {
        let ev = AuditEvent {
            ts_unix_ms: now_unix_ms(),
            kind,
            session_id: session_id.to_string(),
            detail,
        };
        let line = serde_json::to_string(&ev).map_err(|e| TelemetryError::Json(e.to_string()))?;
        self.file
            .write_all(line.as_bytes())
            .and_then(|_| self.file.write_all(b"\n"))
            .and_then(|_| self.file.flush())
            .map_err(|e| TelemetryError::Io(e.to_string()))?;
        Ok(ev)
    }

    /// Lee y parsea un log (diagnóstico accionable / tests).
    pub fn read_all(path: &Path) -> Result<Vec<AuditEvent>, TelemetryError> {
        let f = File::open(path).map_err(|e| TelemetryError::Io(e.to_string()))?;
        let reader = BufReader::new(f);
        let mut out = Vec::new();
        for line in reader.lines() {
            let line = line.map_err(|e| TelemetryError::Io(e.to_string()))?;
            if line.trim().is_empty() {
                continue;
            }
            let ev: AuditEvent = serde_json::from_str(&line)
                .map_err(|e| TelemetryError::MalformedLine(format!("{e}: {line}")))?;
            out.push(ev);
        }
        Ok(out)
    }
}

/// Informe de diagnóstico de una sesión (agregado al cerrar).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionReport {
    pub session_id: String,
    pub started_unix_ms: u64,
    pub ended_unix_ms: Option<u64>,
    pub heartbeats_ok: u64,
    pub rtt_min_ms: Option<u64>,
    pub rtt_last_ms: Option<u64>,
    pub frames_sent: u64,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub files_transferred: u64,
    /// Frames de audio de sesión (RF audio): enviados / recibidos.
    #[serde(default)]
    pub audio_sent: u64,
    #[serde(default)]
    pub audio_recv: u64,
}

impl SessionReport {
    pub fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            started_unix_ms: now_unix_ms(),
            ended_unix_ms: None,
            heartbeats_ok: 0,
            rtt_min_ms: None,
            rtt_last_ms: None,
            frames_sent: 0,
            bytes_sent: 0,
            bytes_recv: 0,
            files_transferred: 0,
            audio_sent: 0,
            audio_recv: 0,
        }
    }

    pub fn record_heartbeat(&mut self, rtt_ms: u64) {
        self.heartbeats_ok += 1;
        self.rtt_last_ms = Some(rtt_ms);
        self.rtt_min_ms = Some(self.rtt_min_ms.map_or(rtt_ms, |m| m.min(rtt_ms)));
    }

    pub fn record_frame_sent(&mut self, bytes: usize) {
        self.frames_sent += 1;
        self.bytes_sent += bytes as u64;
    }

    pub fn record_bytes_recv(&mut self, bytes: usize) {
        self.bytes_recv += bytes as u64;
    }

    pub fn record_file(&mut self) {
        self.files_transferred += 1;
    }

    pub fn record_audio_sent(&mut self, bytes: usize) {
        self.audio_sent += 1;
        self.bytes_sent += bytes as u64;
    }

    pub fn record_audio_recv(&mut self, bytes: usize) {
        self.audio_recv += 1;
        self.bytes_recv += bytes as u64;
    }

    pub fn finish(&mut self) {
        self.ended_unix_ms = Some(now_unix_ms());
    }

    /// Evento de cierre listo para el log (diagnóstico accionable en una línea).
    pub fn disconnect_event(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("anyremote-c7-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn events_roundtrip_as_jsonl() {
        let dir = temp_dir("round");
        let mut log = SessionLog::open(&dir).unwrap();
        log.record(
            AuditEventKind::SessionStarted,
            "s-1",
            json!({"target": "1.2.3.4:21118"}),
        )
        .unwrap();
        log.record(
            AuditEventKind::PermissionGranted,
            "s-1",
            json!({"perms": ["keyboard"]}),
        )
        .unwrap();
        log.record(
            AuditEventKind::Disconnected,
            "s-1",
            json!({"reason": "bye"}),
        )
        .unwrap();

        let events = SessionLog::read_all(log.path()).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].kind, AuditEventKind::SessionStarted);
        assert_eq!(events[0].session_id, "s-1");
        assert!(events[0].ts_unix_ms > 0);
        assert_eq!(events[1].detail["perms"][0], "keyboard");
        assert_eq!(events[2].kind, AuditEventKind::Disconnected);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_lines_are_explicit_errors() {
        let dir = temp_dir("bad");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("audit-1.jsonl");
        std::fs::write(&p, "{\"no\":\"es evento\"}\n").unwrap();
        assert!(matches!(
            SessionLog::read_all(&p),
            Err(TelemetryError::MalformedLine(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn report_tracks_metrics_and_finishes() {
        let mut r = SessionReport::new("s-2");
        r.record_heartbeat(12);
        r.record_heartbeat(5);
        r.record_frame_sent(1000);
        r.record_bytes_recv(500);
        r.record_file();
        assert_eq!(r.heartbeats_ok, 2);
        assert_eq!(r.rtt_min_ms, Some(5));
        assert_eq!(r.rtt_last_ms, Some(5), "last = más reciente (5)");
        assert_eq!(r.bytes_sent, 1000);
        assert_eq!(r.bytes_recv, 500);
        assert_eq!(r.files_transferred, 1);
        assert!(r.ended_unix_ms.is_none());
        r.finish();
        assert!(r.ended_unix_ms.is_some());
        let v = r.disconnect_event();
        assert_eq!(v["session_id"], "s-2");
        assert_eq!(v["heartbeats_ok"], 2);
    }
}
