//! `anyremote-daemon` — binario `anyremoted`: integra los ciclos S0–C8 en una
//! sesión real: TLS+cert-store (S0), autenticación mutua con binding (C2),
//! ARSP hello/permisos (C1), portapapeles (C4), archivos (C5), perfiles (C6,
//! resueltos en main) y auditoría/diagnóstico (C7).
//!
//! Confianza v0.1 (localhost): `server_cert.pem`/`server_key.pem` persistidos en el
//! directorio de configuración; el cliente los valida como root store (validación
//! real, sin verificadores peligrosos). Cross-machine: distribuir `server_cert.pem`.
//! La autenticación de identidad (ed25519+binding) se exige SIEMPRE en capa de
//! aplicación: un MITM rompe el binding de canal y es rechazado.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyremote_clipboard::{ClipboardSyncEngine, MemoryClipboard};
use anyremote_filexfer::{sha256_hex, FileReceiver, FileSender, TransferPolicy};
use anyremote_identity::{Identity, TrustStore};
use anyremote_session::{LinkStats, SessionEvent, SessionState};
use anyremote_telemetry::{AuditEventKind, SessionLog, SessionReport};
use anyremote_transport::arsp::{self, proto};
use anyremote_transport::auth;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

#[derive(Debug)]
pub enum DaemonError {
    Config(String),
    Transport(anyremote_transport::TransportError),
    Profile(anyremote_profiles::ProfileError),
    Telemetry(anyremote_telemetry::TelemetryError),
    Clipboard(anyremote_clipboard::ClipboardError),
    Transfer(anyremote_filexfer::TransferError),
    Io(String),
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl From<anyremote_transport::TransportError> for DaemonError {
    fn from(e: anyremote_transport::TransportError) -> Self {
        Self::Transport(e)
    }
}
impl From<anyremote_profiles::ProfileError> for DaemonError {
    fn from(e: anyremote_profiles::ProfileError) -> Self {
        Self::Profile(e)
    }
}
impl From<anyremote_telemetry::TelemetryError> for DaemonError {
    fn from(e: anyremote_telemetry::TelemetryError) -> Self {
        Self::Telemetry(e)
    }
}
impl From<anyremote_clipboard::ClipboardError> for DaemonError {
    fn from(e: anyremote_clipboard::ClipboardError) -> Self {
        Self::Clipboard(e)
    }
}
impl From<anyremote_filexfer::TransferError> for DaemonError {
    fn from(e: anyremote_filexfer::TransferError) -> Self {
        Self::Transfer(e)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipboardMode {
    Off,
    Memory,
    Win,
}

impl ClipboardMode {
    pub fn parse(s: &str) -> Result<Self, DaemonError> {
        match s {
            "off" => Ok(Self::Off),
            "memory" => Ok(Self::Memory),
            "win" => Ok(Self::Win),
            other => Err(DaemonError::Config(format!(
                "--clipboard inválido: {other} (off|memory|win)"
            ))),
        }
    }
}

pub type EventSink = Box<dyn FnMut(String) + Send + Sync>;

pub struct DaemonConfig {
    /// listen: dirección de enlace. connect: ignorado.
    pub bind: String,
    /// connect: objetivo "host:puerto" (resuelto por main, p.ej. desde perfil).
    pub target: Option<String>,
    /// listen: permisos a SOLICITAR · connect: permisos a CONCEDER (intersección).
    pub grant: Vec<String>,
    pub clipboard: ClipboardMode,
    /// Semilla de texto para modo `memory` (pruebas/wiring; en `win` se ignora).
    pub clipboard_seed: Option<String>,
    /// connect: archivo a enviar tras establecer la sesión (C5).
    pub send_file: Option<PathBuf>,
    pub max_secs: u64,
    /// Directorio de configuración (cert-store + audit/) — por defecto el de perfiles.
    pub config_dir: PathBuf,
    pub on_event: Option<EventSink>,
}

impl DaemonConfig {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            bind: "127.0.0.1:21118".into(),
            target: None,
            grant: vec!["keyboard".into(), "mouse".into()],
            clipboard: ClipboardMode::Win,
            clipboard_seed: None,
            send_file: None,
            max_secs: 600,
            config_dir,
            on_event: None,
        }
    }
}

pub struct RunOutcome {
    pub report: SessionReport,
    pub events: Vec<String>,
    pub clipboard_final: Option<String>,
}

struct Emit {
    events: Vec<String>,
    sink: Option<EventSink>,
}

impl Emit {
    fn emit(&mut self, value: serde_json::Value) {
        let line = value.to_string();
        self.events.push(line.clone());
        if let Some(sink) = self.sink.as_mut() {
            sink(line);
        }
    }
}

fn ev(kind: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("event".into(), serde_json::Value::String(kind.into()));
    if let serde_json::Value::Object(extra) = extra {
        for (k, v) in extra {
            map.insert(k, v);
        }
    }
    serde_json::Value::Object(map)
}

fn make_clipboard(cfg: &DaemonConfig) -> Result<BoxedClip, DaemonError> {
    match cfg.clipboard {
        ClipboardMode::Off => Ok(Box::new(DisabledClipboard)),
        ClipboardMode::Memory => Ok(Box::new(MemoryClipboard {
            current: cfg.clipboard_seed.clone(),
            writes: 0,
        })),
        ClipboardMode::Win => {
            #[cfg(windows)]
            {
                Ok(Box::new(
                    anyremote_clipboard::windows_backend::WinClipboard::new(),
                ))
            }
            #[cfg(not(windows))]
            {
                Err(DaemonError::Config("--clipboard win no soportado aquí".into()))
            }
        }
    }
}

/// Backend inerte (modo `--clipboard off`): política explícita.
struct DisabledClipboard;
impl anyremote_clipboard::ClipboardBackend for DisabledClipboard {
    fn read_text(&mut self) -> Result<Option<String>, anyremote_clipboard::ClipboardError> {
        Ok(None)
    }
    fn write_text(&mut self, _t: &str) -> Result<(), anyremote_clipboard::ClipboardError> {
        Err(anyremote_clipboard::ClipboardError::Denied(
            "clipboard deshabilitado (--clipboard off)".into(),
        ))
    }
    fn description(&self) -> String {
        "disabled".into()
    }
}

type BoxedClip = Box<dyn anyremote_clipboard::ClipboardBackend + Send>;
type Engine = ClipboardSyncEngine<BoxedClip>;

fn session_id_of(identity: &Identity) -> String {
    let id = identity.node_id().to_string();
    format!("ses-{}", &id[..12.min(id.len())])
}

fn transition(state: SessionState, ev: SessionEvent) -> Result<SessionState, DaemonError> {
    state
        .transition(ev)
        .map_err(|e| DaemonError::Config(format!("{e:?}")))
}

/// ¿El error indica que el par desapareció (FIN/RST)? → cierre limpio, no error.
fn conn_gone(e: &anyremote_transport::TransportError) -> bool {
    let s = format!("{e:?}").to_lowercase();
    ["10053", "10054", "1006", "eof", "anulada", "reset", "closed"].iter().any(|k| s.contains(k))
}

/// Lado controlado: acepta UNA sesión (`anyremoted listen`).
pub async fn run_listen(mut cfg: DaemonConfig) -> Result<RunOutcome, DaemonError> {
    let identity = Identity::generate();
    let session_id = session_id_of(&identity);
    let mut emit = Emit {
        events: Vec::new(),
        sink: cfg.on_event.take(),
    };
    let mut log = SessionLog::open(&cfg.config_dir.join("audit"))?;
    let mut report = SessionReport::new(&session_id);

    let (cert, key) = anyremote_transport::cert_store::ensure(&cfg.config_dir)?;
    let srv_cfg = anyremote_transport::server_config_from(cert, key)?;
    let listener = TcpListener::bind(&cfg.bind)
        .await
        .map_err(|e| DaemonError::Io(e.to_string()))?;
    let bind_addr = listener
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();
    emit.emit(ev(
        "listening",
        serde_json::json!({ "bind": bind_addr, "node_id": identity.node_id().to_string() }),
    ));

    let (tcp, peer) = listener
        .accept()
        .await
        .map_err(|e| DaemonError::Io(e.to_string()))?;
    let tls = TlsAcceptor::from(Arc::new(srv_cfg))
        .accept(tcp)
        .await
        .map_err(|e| DaemonError::Io(e.to_string()))?;
    let binding = auth::binding_of_server(&tls)?;
    let mut ch = arsp::Channel::new(tls);

    // C2: autenticación mutua con channel binding (trust store vacío = desconocido).
    let mut gate = auth::AuthGate::new();
    let trust = TrustStore::new();
    let peer_id = auth::recv_validate_auth(&mut ch, &mut gate, &trust, &binding).await?;
    auth::send_auth(&mut ch, &identity, &binding).await?;
    log.record(
        AuditEventKind::SessionEstablished,
        &session_id,
        serde_json::json!({"peer": peer_id.node_id.to_string(), "peer_trusted": peer_id.trusted, "peer_addr": peer.to_string()}),
    )?;
    emit.emit(ev(
        "session_active",
        serde_json::json!({"role": "controlled", "peer": peer_id.node_id.to_string(), "session_id": session_id}),
    ));

    // C1: hello/welcome + permisos (el controlado SOLICITA; D-07 por defecto nada activo).
    let hello = ch.recv().await?;
    arsp::check_version(&hello).map_err(DaemonError::Config)?;
    ch.send(arsp::welcome(&identity.node_id().to_string())).await?;
    let requested = cfg.grant.clone();
    ch.send(proto::envelope::Payload::PermissionRequest(
        proto::PermissionRequest {
            permissions: requested,
        },
    ))
    .await?;

    let _state = transition(SessionState::Handshaking, SessionEvent::HandshakeOk)?;

    let resp = ch.recv().await?;
    let granted = match resp.payload {
        Some(proto::envelope::Payload::PermissionResponse(r)) => r,
        other => {
            return Err(DaemonError::Config(format!(
                "esperaba PermissionResponse: {other:?}"
            )))
        }
    };
    if !granted.granted || granted.permissions.is_empty() {
        log.record(
            AuditEventKind::PermissionDenied,
            &session_id,
            serde_json::json!({"from": "controller"}),
        )?;
        emit.emit(ev("closed", serde_json::json!({"reason": "permission_denied"})));
        report.finish();
        return Ok(RunOutcome {
            report,
            events: emit.events,
            clipboard_final: None,
        });
    }
    log.record(
        AuditEventKind::PermissionGranted,
        &session_id,
        serde_json::json!({"perms": granted.permissions}),
    )?;

    active_controlled(&mut ch, &mut log, &mut report, &mut emit, &cfg, &session_id).await
}

async fn active_controlled(
    ch: &mut arsp::Channel<tokio_rustls::server::TlsStream<TcpStream>>,
    log: &mut SessionLog,
    report: &mut SessionReport,
    emit: &mut Emit,
    cfg: &DaemonConfig,
    session_id: &str,
) -> Result<RunOutcome, DaemonError> {
    let mut engine = Engine::new(make_clipboard(cfg)?);
    let mut files = FileReceiver::new(TransferPolicy::default());
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(cfg.max_secs);
    let mut clip_tick = tokio::time::interval(std::time::Duration::from_millis(500));
    let state = SessionState::Active;

    loop {
        tokio::select! {
            env = ch.recv() => {
                let env = match env {
                    Ok(e) => e,
                    Err(e) if conn_gone(&e) => {
                        log.record(AuditEventKind::Disconnected, session_id,
                            serde_json::json!({"reason":"connection_gone","role":"peer"}))?;
                        emit.emit(ev("closed", serde_json::json!({"reason": "connection_gone"})));
                        break;
                    }
                    Err(e) => return Err(e.into()),
                };
                match env.payload {
                    Some(proto::envelope::Payload::Heartbeat(h)) if !h.echo => {
                        ch.send(proto::envelope::Payload::Heartbeat(proto::Heartbeat {
                            seq: h.seq,
                            sent_unix_ms: h.sent_unix_ms,
                            echo: true,
                        })).await?;
                        report.record_bytes_recv(0);
                    }
                    Some(proto::envelope::Payload::Heartbeat(_)) => {}
                    Some(proto::envelope::Payload::Clipboard(w)) => {
                        let content = arsp::clipboard_from_wire(w)?;
                        if engine.apply_remote(&content)? {
                            log.record(AuditEventKind::ClipboardSync, session_id,
                                serde_json::json!({"dir":"in","bytes": content.text.len()}))?;
                            report.record_bytes_recv(content.text.len());
                            emit.emit(ev("clipboard_synced", serde_json::json!({"dir":"in"})));
                        }
                    }
                    Some(proto::envelope::Payload::FileOffer(o)) => {
                        let meta = arsp::file_offer_from_wire(o)?;
                        let (tid, size) = (meta.transfer_id, meta.size);
                        match files.accept_offer(meta.clone()) {
                            Ok(()) => {
                                while files.next_offset(tid)? < size {
                                    let c = ch.recv().await?;
                                    if let Some(proto::envelope::Payload::FileChunk(fc)) = c.payload {
                                        files.write_chunk(fc.transfer_id, fc.offset, &fc.data)?;
                                    } else {
                                        return Err(DaemonError::Config("esperaba FileChunk".into()));
                                    }
                                }
                                let data = files.finish(tid)?;
                                let hex = sha256_hex(&data);
                                ch.send(proto::envelope::Payload::FileDone(
                                    arsp::file_done_to_wire(tid, true, &hex),
                                )).await?;
                                report.record_file();
                                log.record(AuditEventKind::FileTransfer, session_id,
                                    serde_json::json!({"dir":"in","name": meta.name, "bytes": data.len(), "sha256": hex}))?;
                                emit.emit(ev("file_received", serde_json::json!({"name": meta.name, "bytes": data.len()})));
                            }
                            Err(e) => {
                                ch.send(proto::envelope::Payload::FileDone(
                                    arsp::file_done_to_wire(tid, false, &format!("{e:?}")),
                                )).await?;
                                log.record(AuditEventKind::Error, session_id,
                                    serde_json::json!({"file_reject": format!("{e:?}")}))?;
                            }
                        }
                    }
                    Some(proto::envelope::Payload::Bye(b)) => {
                        log.record(AuditEventKind::Disconnected, session_id,
                            serde_json::json!({"reason": b.reason.clone(), "role":"controlled"}))?;
                        emit.emit(ev("closed", serde_json::json!({"reason": b.reason})));
                        let closing = transition(state, SessionEvent::StartClose)?;
                        let _ = transition(closing, SessionEvent::Closed)?;
                        break;
                    }
                    other => return Err(DaemonError::Config(format!("payload inesperado: {other:?}"))),
                }
            }
            _ = clip_tick.tick(), if cfg.clipboard != ClipboardMode::Off => {
                if let Some(content) = engine.detect_local_change()? {
                    ch.send(proto::envelope::Payload::Clipboard(
                        arsp::clipboard_to_wire(&content)?,
                    )).await?;
                    log.record(AuditEventKind::ClipboardSync, session_id,
                        serde_json::json!({"dir":"out","bytes": content.text.len()}))?;
                    emit.emit(ev("clipboard_synced", serde_json::json!({"dir":"out"})));
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                ch.send(proto::envelope::Payload::Bye(proto::Bye { reason: "max_secs".into() })).await?;
                // Gracia: dar tiempo a que el par procese el Bye antes de cerrar el socket.
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                log.record(AuditEventKind::Disconnected, session_id,
                    serde_json::json!({"reason":"max_secs","role":"controlled"}))?;
                emit.emit(ev("closed", serde_json::json!({"reason": "max_secs"})));
                break;
            }
        }
    }

    report.finish();
    log.record(AuditEventKind::Disconnected, session_id, report.disconnect_event())?;
    let clipboard_final = engine.text()?;
    Ok(RunOutcome {
        report: report.clone(),
        events: emit.events.clone(),
        clipboard_final,
    })
}

/// Lado controlador: inicia UNA sesión (`anyremoted connect <target>`).
pub async fn run_connect(mut cfg: DaemonConfig) -> Result<RunOutcome, DaemonError> {
    let identity = Identity::generate();
    let session_id = session_id_of(&identity);
    let mut emit = Emit {
        events: Vec::new(),
        sink: cfg.on_event.take(),
    };
    let mut log = SessionLog::open(&cfg.config_dir.join("audit"))?;
    let mut report = SessionReport::new(&session_id);
    let target = cfg
        .target
        .clone()
        .ok_or_else(|| DaemonError::Config("falta el objetivo (o --profile)".into()))?;

    log.record(
        AuditEventKind::SessionStarted,
        &session_id,
        serde_json::json!({"target": target.clone()}),
    )?;
    emit.emit(ev("connecting", serde_json::json!({ "target": target.clone(), "node_id": identity.node_id().to_string() })));

    // Raíz = cert persistido del listen (v0.1 localhost). Si no existe, se crea
    // (orden de arranque indiferente en la misma máquina: quien llegue primero lo genera).
    let (root, _) = anyremote_transport::cert_store::ensure(&cfg.config_dir)?;
    let cli_cfg = anyremote_transport::client_config(&root)?;

    let tcp = TcpStream::connect(&target)
        .await
        .map_err(|e| DaemonError::Io(e.to_string()))?;
    let sni = rustls::pki_types::ServerName::try_from("localhost")
        .map_err(|e| DaemonError::Config(format!("SNI: {e}")))?;
    let tls = TlsConnector::from(Arc::new(cli_cfg))
        .connect(sni, tcp)
        .await
        .map_err(|e| DaemonError::Io(e.to_string()))?;
    let binding = auth::binding_of_client(&tls)?;
    let mut ch = arsp::Channel::new(tls);

    // C2: autenticación mutua.
    auth::send_auth(&mut ch, &identity, &binding).await?;
    let mut gate = auth::AuthGate::new();
    let trust = TrustStore::new();
    let peer_id = auth::recv_validate_auth(&mut ch, &mut gate, &trust, &binding).await?;
    log.record(
        AuditEventKind::SessionEstablished,
        &session_id,
        serde_json::json!({"peer": peer_id.node_id.to_string(), "peer_trusted": peer_id.trusted}),
    )?;
    emit.emit(ev(
        "session_active",
        serde_json::json!({"role": "controller", "peer": peer_id.node_id.to_string(), "session_id": session_id}),
    ));

    // C1: hello → welcome → solicitud → concesión (intersección con --grant).
    ch.send(arsp::hello(&identity.node_id().to_string(), &session_id)).await?;
    let welcome = ch.recv().await?;
    arsp::check_version(&welcome).map_err(DaemonError::Config)?;
    let req = ch.recv().await?;
    let requested = match req.payload {
        Some(proto::envelope::Payload::PermissionRequest(r)) => r.permissions,
        other => {
            return Err(DaemonError::Config(format!(
                "esperaba PermissionRequest: {other:?}"
            )))
        }
    };
    let allowed: Vec<String> = requested
        .into_iter()
        .filter(|p| cfg.grant.contains(p))
        .collect();
    let grant_ok = !allowed.is_empty();
    ch.send(proto::envelope::Payload::PermissionResponse(
        proto::PermissionResponse {
            granted: grant_ok,
            permissions: allowed.clone(),
        },
    ))
    .await?;
    if !grant_ok {
        log.record(
            AuditEventKind::PermissionDenied,
            &session_id,
            serde_json::json!({"reason": "interseccion_vacia"}),
        )?;
        emit.emit(ev("closed", serde_json::json!({"reason": "permission_denied"})));
        report.finish();
        return Ok(RunOutcome {
            report,
            events: emit.events,
            clipboard_final: None,
        });
    }
    log.record(AuditEventKind::PermissionGranted, &session_id, serde_json::json!({"perms": allowed}))?;

    active_controller(&mut ch, &mut log, &mut report, &mut emit, &cfg, &session_id, &peer_id.node_id.to_string()).await
}

async fn active_controller(
    ch: &mut arsp::Channel<tokio_rustls::client::TlsStream<TcpStream>>,
    log: &mut SessionLog,
    report: &mut SessionReport,
    emit: &mut Emit,
    cfg: &DaemonConfig,
    session_id: &str,
    peer: &str,
) -> Result<RunOutcome, DaemonError> {
    let mut engine = Engine::new(make_clipboard(cfg)?);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(cfg.max_secs);
    let mut hb_tick = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut clip_tick = tokio::time::interval(std::time::Duration::from_millis(500));
    let mut stats = LinkStats::default();
    let mut next_seq: u64 = 1;
    let state = SessionState::Active;

    // C5: envío de archivo opcional tras activar.
    let mut pending_file: Option<(u64, String, String)> = None;
    if let Some(path) = cfg.send_file.clone() {
        let data = std::fs::read(&path).map_err(|e| DaemonError::Io(format!("{path:?}: {e}")))?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("archivo.bin")
            .to_string();
        let sender = FileSender::new(TransferPolicy::default());
        let meta = sender.offer(1, &name, &data)?;
        let hex = sha256_hex(&data);
        ch.send(proto::envelope::Payload::FileOffer(arsp::file_offer_to_wire(&meta))).await?;
        for (off, chunk) in sender.chunks_from(&data, 0, 32 * 1024) {
            ch.send(proto::envelope::Payload::FileChunk(arsp::file_chunk_to_wire(
                meta.transfer_id,
                off,
                chunk,
            ))).await?;
        }
        report.record_frame_sent(data.len());
        pending_file = Some((meta.transfer_id, hex, name));
        emit.emit(ev("file_sent", serde_json::json!({"name": name_hint(&pending_file)})));
    }

    loop {
        tokio::select! {
            env = ch.recv() => {
                let env = match env {
                    Ok(e) => e,
                    Err(e) if conn_gone(&e) => {
                        log.record(AuditEventKind::Disconnected, session_id,
                            serde_json::json!({"reason":"connection_gone","role":"peer"}))?;
                        emit.emit(ev("closed", serde_json::json!({"reason": "connection_gone"})));
                        break;
                    }
                    Err(e) => return Err(e.into()),
                };
                match env.payload {
                    Some(proto::envelope::Payload::Heartbeat(h)) if h.echo => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0);
                        let rtt = (now - h.sent_unix_ms).max(0) as u64;
                        stats.record_rtt(rtt);
                        report.record_heartbeat(rtt);
                        emit.emit(ev("heartbeat", serde_json::json!({"seq": h.seq, "rtt_ms": rtt})));
                    }
                    Some(proto::envelope::Payload::Heartbeat(_)) => {}
                    Some(proto::envelope::Payload::Clipboard(w)) => {
                        let content = arsp::clipboard_from_wire(w)?;
                        if engine.apply_remote(&content)? {
                            log.record(AuditEventKind::ClipboardSync, session_id,
                                serde_json::json!({"dir":"in","bytes": content.text.len()}))?;
                            emit.emit(ev("clipboard_synced", serde_json::json!({"dir":"in"})));
                        }
                    }
                    Some(proto::envelope::Payload::FileDone(d)) => {
                        if let Some((tid, hex, name)) = pending_file.take() {
                            if d.transfer_id == tid {
                                if d.ok && d.detail == hex {
                                    report.record_file();
                                    log.record(AuditEventKind::FileTransfer, session_id,
                                        serde_json::json!({"dir":"out","name": name.clone(), "sha256": hex.clone(), "ack": true}))?;
                                    emit.emit(ev("file_ack", serde_json::json!({"name": name, "sha256": hex})));
                                } else {
                                    log.record(AuditEventKind::Error, session_id,
                                        serde_json::json!({"file_nack": d.detail.clone()}))?;
                                    emit.emit(ev("error", serde_json::json!({"detail": format!("file nack: {}", d.detail)})));
                                }
                            }
                        }
                    }
                    Some(proto::envelope::Payload::Bye(b)) => {
                        log.record(AuditEventKind::Disconnected, session_id,
                            serde_json::json!({"reason": b.reason.clone(), "role":"controller", "peer": peer}))?;
                        emit.emit(ev("closed", serde_json::json!({"reason": b.reason})));
                        let closing = transition(state, SessionEvent::StartClose)?;
                        let _ = transition(closing, SessionEvent::Closed)?;
                        break;
                    }
                    other => return Err(DaemonError::Config(format!("payload inesperado: {other:?}"))),
                }
            }
            _ = hb_tick.tick() => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                ch.send(proto::envelope::Payload::Heartbeat(proto::Heartbeat {
                    seq: next_seq,
                    sent_unix_ms: now,
                    echo: false,
                })).await?;
                next_seq += 1;
            }
            _ = clip_tick.tick(), if cfg.clipboard != ClipboardMode::Off => {
                if let Some(content) = engine.detect_local_change()? {
                    ch.send(proto::envelope::Payload::Clipboard(
                        arsp::clipboard_to_wire(&content)?,
                    )).await?;
                    log.record(AuditEventKind::ClipboardSync, session_id,
                        serde_json::json!({"dir":"out","bytes": content.text.len()}))?;
                    emit.emit(ev("clipboard_synced", serde_json::json!({"dir":"out"})));
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                ch.send(proto::envelope::Payload::Bye(proto::Bye { reason: "max_secs".into() })).await?;
                // Gracia: dar tiempo a que el par procese el Bye antes de cerrar el socket.
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                log.record(AuditEventKind::Disconnected, session_id,
                    serde_json::json!({"reason":"max_secs","role":"controller"}))?;
                emit.emit(ev("closed", serde_json::json!({"reason": "max_secs"})));
                break;
            }
        }
    }

    log.record(
        AuditEventKind::HeartbeatSummary,
        session_id,
        serde_json::json!({
            "heartbeats_ok": stats.heartbeats_ok,
            "rtt_min_ms": stats.min_rtt_ms,
            "rtt_last_ms": stats.last_rtt_ms,
        }),
    )?;
    report.finish();
    log.record(AuditEventKind::Disconnected, session_id, report.disconnect_event())?;
    let clipboard_final = engine.text()?;
    Ok(RunOutcome {
        report: report.clone(),
        events: emit.events.clone(),
        clipboard_final,
    })
}

fn name_hint(pending: &Option<(u64, String, String)>) -> String {
    pending
        .as_ref()
        .map(|(_, _, n)| n.clone())
        .unwrap_or_default()
}

/// Resuelve `--profile <alias>` contra el almacén de perfiles (C6).
pub fn resolve_profile(alias: &str, config_dir: &Path) -> Result<String, DaemonError> {
    let file = config_dir.join(anyremote_profiles::keys::PROFILES_FILE);
    let store = anyremote_profiles::ProfilesStore::at(file);
    store
        .load()?
        .into_iter()
        .find(|p| p.id == alias)
        .map(|p| p.resolve())
        .ok_or_else(|| DaemonError::Profile(anyremote_profiles::ProfileError::NotFound(alias.into())))
}
