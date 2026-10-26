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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyremote_clipboard::{ClipboardSyncEngine, MemoryClipboard};
use anyremote_filexfer::{sha256_hex, FileReceiver, FileSender};
use anyremote_identity::{Identity, TrustStore};
use anyremote_codec::VideoDecoder;
use anyremote_session::{LinkStats, SessionEvent, SessionState};
use anyremote_telemetry::{AuditEventKind, SessionLog, SessionReport};
use anyremote_transport::arsp::{self, proto};
use anyremote_transport::auth;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::{TlsAcceptor, TlsConnector};

pub mod preview;
pub mod video;

use video::{spawn_video_task, VideoMsg, VideoMode};

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
    /// Diagnóstico: primeras lecturas OK y después fallos simulados
    /// (reproduce bloqueos reales de portapapeles para tests de resiliencia).
    Flaky,
}

impl ClipboardMode {
    pub fn parse(s: &str) -> Result<Self, DaemonError> {
        match s {
            "off" => Ok(Self::Off),
            "memory" => Ok(Self::Memory),
            "win" => Ok(Self::Win),
            "flaky" => Ok(Self::Flaky),
            other => Err(DaemonError::Config(format!(
                "--clipboard inválido: {other} (off|memory|win|flaky)"
            ))),
        }
    }
}

pub type EventSink = Box<dyn FnMut(String) + Send + Sync>;

/// Versión del binario daemon (viajada en los eventos para diagnóstico de campo).
pub const DAEMON_VERSION: &str = env!("CARGO_PKG_VERSION");

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
    /// Hito M2: envío de vídeo desde el lado `listen` (el receptor siempre decodifica).
    pub video: VideoMode,
    pub video_size: (u32, u32),
    pub video_fps: u64,
    pub codec: anyremote_codec::CodecFormat,
    /// Política de un solo sentido para archivos (patrón OPTION_ONE_WAY_FILE_TRANSFER):
    /// solo controlador → controlado; el controlador RECHAZA ofertas entrantes.
    pub one_way_files: bool,
    /// Fuente de cierre externa (tests): envía "bye" al canal de cierre.
    pub bye_source: Option<tokio::sync::mpsc::Receiver<String>>,
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
            video: VideoMode::Off,
            video_size: (960, 540),
            video_fps: 10,
            codec: anyremote_codec::CodecFormat::Vp9,
            one_way_files: false,
            bye_source: None,
        }
    }
}

/// Política de transferencia derivada de la configuración del daemon.
fn file_policy(cfg: &DaemonConfig) -> anyremote_filexfer::TransferPolicy {
    anyremote_filexfer::TransferPolicy {
        one_way: cfg.one_way_files,
        ..anyremote_filexfer::TransferPolicy::default()
    }
}

/// Fuente de cierre: inyectada (tests) o stdin del proceso (`bye` / fin de archivo).
fn build_bye_source(cfg: &mut DaemonConfig) -> tokio::sync::mpsc::Receiver<String> {
    if let Some(rx) = cfg.bye_source.take() {
        return rx;
    }
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(2);
    tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx.send(line).await.is_err() {
                break;
            }
        }
    });
    rx
}

/// Detiene la tarea de vídeo al salir del ámbito de la sesión.
struct StopGuard(Arc<AtomicBool>);
impl Drop for StopGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
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
        ClipboardMode::Flaky => Ok(Box::new(FlakyClipboard {
            seed: cfg
                .clipboard_seed
                .clone()
                .unwrap_or_else(|| "semilla".into()),
            reads: 0,
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

/// Diagnóstico: simula bloqueos reales de portapapeles (primeras ops OK, luego error).
struct FlakyClipboard {
    seed: String,
    reads: u32,
}

impl anyremote_clipboard::ClipboardBackend for FlakyClipboard {
    fn read_text(&mut self) -> Result<Option<String>, anyremote_clipboard::ClipboardError> {
        self.reads += 1;
        if self.reads <= 1 {
            return Ok(Some(self.seed.clone()));
        }
        Err(anyremote_clipboard::ClipboardError::Platform(
            "bloqueo simulado de portapapeles (modo flaky)".into(),
        ))
    }

    fn write_text(&mut self, _text: &str) -> Result<(), anyremote_clipboard::ClipboardError> {
        Err(anyremote_clipboard::ClipboardError::Platform(
            "bloqueo simulado de portapapeles (modo flaky)".into(),
        ))
    }

    fn description(&self) -> String {
        "flaky".into()
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

/// Resultado del establecimiento con un cliente.
enum Established {
    /// Sesión autenticada y con permisos → pasar a `active_controlled`.
    Active(arsp::Channel<tokio_rustls::server::TlsStream<TcpStream>>),
    /// Permiso denegado: cliente descartado; el listener sigue vivo.
    Denied,
}

/// Handshake con UN cliente ya aceptado: bootstrap → TLS → auth (C2) → permisos (C1).
/// Todo fallo aquí es del cliente (el caller continúa aceptando).
async fn establish_controlled(
    mut tcp: TcpStream,
    peer: std::net::SocketAddr,
    acceptor: &TlsAcceptor,
    identity: &Identity,
    session_id: &str,
    cfg: &DaemonConfig,
    log: &mut SessionLog,
    emit: &mut Emit,
) -> Result<Established, DaemonError> {
    // Bootstrap de raíz (PEM público) antes del TLS → confianza cross-machine.
    if let Some(pem) = anyremote_transport::cert_store::read_pem(&cfg.config_dir)? {
        anyremote_transport::cert_store::write_bootstrap(&mut tcp, &pem).await?;
    }
    let tls = acceptor
        .accept(tcp)
        .await
        .map_err(|e| DaemonError::Io(format!("tls del cliente {peer}: {e}")))?;
    let binding = auth::binding_of_server(&tls)?;
    let mut ch = arsp::Channel::new(tls);

    // C2: autenticación mutua con channel binding (trust store vacío = desconocido).
    let mut gate = auth::AuthGate::new();
    let trust = TrustStore::new();
    let peer_id = auth::recv_validate_auth(&mut ch, &mut gate, &trust, &binding).await?;
    auth::send_auth(&mut ch, &identity, &binding).await?;
    log.record(
        AuditEventKind::SessionEstablished,
        session_id,
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
            session_id,
            serde_json::json!({"from": "controller", "peer": peer.to_string()}),
        )?;
        emit.emit(ev("client_failed", serde_json::json!({"peer": peer.to_string(), "detail": "permission_denied"})));
        return Ok(Established::Denied);
    }
    log.record(
        AuditEventKind::PermissionGranted,
        session_id,
        serde_json::json!({"perms": granted.permissions}),
    )?;
    Ok(Established::Active(ch))
}

/// Lado controlado: acepta sesiones (`anyremoted listen`).
///
/// **Resiliencia (prueba cross-device)**: un cliente que falle en cualquier etapa
/// (TLS, auth, secuencia, silencio >15 s) se audita como `client_failed` y el
/// listener **sigue aceptando**; solo termina con una sesión completada o al
/// agotar `--max-secs` de vida del listener.
pub async fn run_listen(mut cfg: DaemonConfig) -> Result<RunOutcome, DaemonError> {
    let identity = Identity::generate();
    let session_id = session_id_of(&identity);
    let mut emit = Emit {
        events: Vec::new(),
        sink: cfg.on_event.take(),
    };
    let mut log = SessionLog::open(&cfg.config_dir.join("audit"))?;
    let mut report = SessionReport::new(&session_id);
    let bye_rx = build_bye_source(&mut cfg);

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
        serde_json::json!({ "bind": bind_addr, "node_id": identity.node_id().to_string(), "daemon": DAEMON_VERSION }),
    ));

    let acceptor = TlsAcceptor::from(Arc::new(srv_cfg));
    let listen_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(cfg.max_secs);

    loop {
        let remaining = listen_deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            log.record(
                AuditEventKind::Disconnected,
                &session_id,
                serde_json::json!({"reason":"listen_timeout","role":"listener"}),
            )?;
            emit.emit(ev("closed", serde_json::json!({"reason": "listen_timeout"})));
            report.finish();
            break;
        }

        let (tcp, peer) = match tokio::time::timeout(remaining, listener.accept()).await {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return Err(DaemonError::Io(e.to_string())), // fallo del socket local
            Err(_idle) => {
                log.record(
                    AuditEventKind::Disconnected,
                    &session_id,
                    serde_json::json!({"reason":"listen_timeout","role":"listener"}),
                )?;
                emit.emit(ev("closed", serde_json::json!({"reason": "listen_timeout"})));
                report.finish();
                break;
            }
        };

        let attempt = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            establish_controlled(
                tcp,
                peer,
                &acceptor,
                &identity,
                &session_id,
                &cfg,
                &mut log,
                &mut emit,
            ),
        )
        .await;

        match attempt {
            Ok(Ok(Established::Active(mut ch))) => {
                // Errores de sesión en curso → cierre limpio auditable (no matan el listener como proceso con pánico).
                return match active_controlled(&mut ch, &mut log, &mut report, &mut emit, &cfg, &session_id, bye_rx).await {
                    Ok(o) => Ok(o),
                    Err(e) => {
                        log.record(AuditEventKind::Error, &session_id,
                            serde_json::json!({"session_error": e.to_string()}))?;
                        emit.emit(ev("closed", serde_json::json!({"reason": format!("session_error: {e}")})));
                        report.finish();
                        log.record(AuditEventKind::Disconnected, &session_id, report.disconnect_event())?;
                        Ok(RunOutcome { report, events: emit.events, clipboard_final: None })
                    }
                };
            }
            Ok(Ok(Established::Denied)) => {
                // Cliente descartado tras auditoría → sigue escuchando.
                continue;
            }
            Ok(Err(e)) => {
                // Cliente falló (TLS/auth/secuencia): audita y NO mata el listener.
                log.record(
                    AuditEventKind::Error,
                    &session_id,
                    serde_json::json!({"client": peer.to_string(), "error": e.to_string()}),
                )?;
                emit.emit(ev(
                    "client_failed",
                    serde_json::json!({"peer": peer.to_string(), "detail": e.to_string()}),
                ));
                continue;
            }
            Err(_elapsed) => {
                log.record(
                    AuditEventKind::Error,
                    &session_id,
                    serde_json::json!({"client": peer.to_string(), "error": "handshake_timeout_15s"}),
                )?;
                emit.emit(ev(
                    "client_failed",
                    serde_json::json!({"peer": peer.to_string(), "detail": "handshake_timeout (15s)"}),
                ));
                continue;
            }
        }
    }

    log.record(
        AuditEventKind::Disconnected,
        &session_id,
        report.disconnect_event(),
    )?;
    let clipboard_final = None;
    Ok(RunOutcome {
        report,
        events: emit.events,
        clipboard_final,
    })
}

/// Resultado de recibir una oferta completa: (contenido, transfer_id).
/// Error: (transfer_id, detalle) para poder enviar el nack.
async fn receive_file_offer<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    ch: &mut arsp::Channel<T>,
    files: &mut FileReceiver,
    o: proto::FileOffer,
) -> Result<(Vec<u8>, u64), (u64, String)> {
    let meta = arsp::file_offer_from_wire(o).map_err(|e| (0u64, e.to_string()))?;
    let tid = meta.transfer_id;
    let size = meta.size;
    files.accept_offer(meta).map_err(|e| (tid, e.to_string()))?;
    // Aceptación + offset de reanudación para el emisor (C5 v2 / reanudación entre sesiones).
    let resume = files.next_offset(tid).map_err(|e| (tid, e.to_string()))?;
    ch.send(proto::envelope::Payload::FileReady(proto::FileReady {
        transfer_id: tid,
        resume_offset: resume,
    }))
    .await
    .map_err(|e| (tid, e.to_string()))?;
    while files.next_offset(tid).map_err(|e| (tid, e.to_string()))? < size {
        let c = ch.recv().await.map_err(|e| (tid, e.to_string()))?;
        match c.payload {
            Some(proto::envelope::Payload::FileChunk(fc)) => {
                files
                    .write_chunk(fc.transfer_id, fc.offset, &fc.data)
                    .map_err(|e| (tid, e.to_string()))?;
            }
            _ => return Err((tid, "esperaba FileChunk".into())),
        }
    }
    files
        .finish(tid)
        .map_err(|e| (tid, e.to_string()))
        .map(|data| (data, tid))
}

async fn active_controlled(
    ch: &mut arsp::Channel<tokio_rustls::server::TlsStream<TcpStream>>,
    log: &mut SessionLog,
    report: &mut SessionReport,
    emit: &mut Emit,
    cfg: &DaemonConfig,
    session_id: &str,
    mut bye_rx: tokio::sync::mpsc::Receiver<String>,
) -> Result<RunOutcome, DaemonError> {
    let mut engine = Engine::new(make_clipboard(cfg)?);
    let mut files = FileReceiver::new_with_store(file_policy(cfg), cfg.config_dir.join("transfers"));
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(cfg.max_secs);
    let mut clip_tick = tokio::time::interval(std::time::Duration::from_millis(500));
    let state = SessionState::Active;
    // Un fallo de portapapeles NO mata la sesión: se desactiva el subsistema (v0.1.4).
    let mut clip_off = false;

    // Hito M2: vídeo hacia el canal + preview local de la propia pantalla (solo listen).
    let (video_tx, mut video_rx) = mpsc::channel::<VideoMsg>(3);
    let stop = Arc::new(AtomicBool::new(false));
    let _stop_guard = StopGuard(stop.clone());
    let mut video_done = false;
    let mut bye_done = false;
    if cfg.video != VideoMode::Off {
        let (preview_srv, preview_port, _preview_task) = preview::PreviewServer::start();
        emit.emit(ev("preview", serde_json::json!({ "port": preview_port })));
        emit.emit(ev("video_start", serde_json::json!({
            "mode": format!("{:?}", cfg.video),
            "width": cfg.video_size.0, "height": cfg.video_size.1,
            "fps": cfg.video_fps, "codec": format!("{:?}", cfg.codec),
        })));
        spawn_video_task(cfg.video, cfg.video_size, cfg.video_fps, cfg.codec, video_tx, preview_srv, stop);
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
                    Some(proto::envelope::Payload::Heartbeat(h)) if !h.echo => {
                        if let Err(e) = ch.send(proto::envelope::Payload::Heartbeat(proto::Heartbeat {
                            seq: h.seq,
                            sent_unix_ms: h.sent_unix_ms,
                            echo: true,
                        })).await {
                            if conn_gone(&e) {
                                log.record(AuditEventKind::Disconnected, session_id,
                                    serde_json::json!({"reason":"connection_gone","role":"peer"}))?;
                                emit.emit(ev("closed", serde_json::json!({"reason": "connection_gone"})));
                                break;
                            }
                            return Err(e.into());
                        }
                        report.record_bytes_recv(0);
                    }
                    Some(proto::envelope::Payload::Heartbeat(_)) => {}
                    Some(proto::envelope::Payload::Clipboard(w)) => {
                        let content = arsp::clipboard_from_wire(w)?;
                        match engine.apply_remote(&content) {
                            Ok(true) => {
                                log.record(AuditEventKind::ClipboardSync, session_id,
                                    serde_json::json!({"dir":"in","bytes": content.text.len()}))?;
                                report.record_bytes_recv(content.text.len());
                                emit.emit(ev("clipboard_synced", serde_json::json!({"dir":"in"})));
                            }
                            Ok(false) => {}
                            Err(e) => {
                                // Resiliencia: el portapapeles local falló → se desactiva, la sesión sigue.
                                clip_off = true;
                                log.record(AuditEventKind::Error, session_id,
                                    serde_json::json!({"clipboard": format!("{e:?}"), "dir":"in"}))?;
                                emit.emit(ev("clipboard_error", serde_json::json!({"dir":"in","detail": format!("{e:?}")})));
                            }
                        }
                    }
                    Some(proto::envelope::Payload::FileOffer(o)) => {
                        match receive_file_offer(ch, &mut files, o).await {
                            Ok(data) => {
                                let tid_meta = data.1;
                                let data = data.0;
                                let hex = sha256_hex(&data);
                                let _ = ch.send(proto::envelope::Payload::FileDone(
                                    arsp::file_done_to_wire(tid_meta, true, &hex),
                                )).await;
                                report.record_file();
                                log.record(AuditEventKind::FileTransfer, session_id,
                                    serde_json::json!({"dir":"in","bytes": data.len(), "sha256": hex}))?;
                                emit.emit(ev("file_received", serde_json::json!({"bytes": data.len()})));
                            }
                            Err((tid, detail)) => {
                                // Resiliencia: transferencia fallida → nack + sesión sigue.
                                let _ = ch.send(proto::envelope::Payload::FileDone(
                                    arsp::file_done_to_wire(tid, false, &detail),
                                )).await;
                                log.record(AuditEventKind::Error, session_id,
                                    serde_json::json!({"file": detail.clone()}))?;
                                emit.emit(ev("file_error", serde_json::json!({"detail": detail})));
                            }
                        }
                    }
                    Some(proto::envelope::Payload::VideoFrame(_)) => {
                        // El controlado no recibe vídeo (M2): ignora con seguridad.
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
            _ = clip_tick.tick(), if cfg.clipboard != ClipboardMode::Off && !clip_off => {
                match engine.detect_local_change() {
                    Ok(Some(content)) => {
                        match arsp::clipboard_to_wire(&content) {
                            Ok(w) => match ch.send(proto::envelope::Payload::Clipboard(w)).await {
                                Ok(()) => {
                                    log.record(AuditEventKind::ClipboardSync, session_id,
                                        serde_json::json!({"dir":"out","bytes": content.text.len()}))?;
                                    emit.emit(ev("clipboard_synced", serde_json::json!({"dir":"out"})));
                                }
                                Err(e) if conn_gone(&e) => {
                                    log.record(AuditEventKind::Disconnected, session_id,
                                        serde_json::json!({"reason":"connection_gone","role":"peer"}))?;
                                    emit.emit(ev("closed", serde_json::json!({"reason": "connection_gone"})));
                                    break;
                                }
                                Err(e) => {
                                    clip_off = true;
                                    log.record(AuditEventKind::Error, session_id,
                                        serde_json::json!({"clipboard": format!("{e:?}"), "dir":"out"}))?;
                                    emit.emit(ev("clipboard_error", serde_json::json!({"dir":"out","detail": format!("{e:?}")})));
                                }
                            },
                            Err(e) => {
                                clip_off = true;
                                log.record(AuditEventKind::Error, session_id,
                                    serde_json::json!({"clipboard": format!("{e:?}"), "dir":"out"}))?;
                                emit.emit(ev("clipboard_error", serde_json::json!({"dir":"out","detail": format!("{e:?}")})));
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        // Resiliencia: lectura local falló → desactiva portapapeles, la sesión sigue.
                        clip_off = true;
                        log.record(AuditEventKind::Error, session_id,
                            serde_json::json!({"clipboard": format!("{e:?}"), "dir":"out"}))?;
                        emit.emit(ev("clipboard_error", serde_json::json!({"dir":"out","detail": format!("{e:?}")})));
                    }
                }
            }
            msg = video_rx.recv(), if cfg.video != VideoMode::Off && !video_done => {
                let Some(msg) = msg else { video_done = true; continue; };
                match msg {
                    VideoMsg::Event(v) => emit.emit(v),
                    VideoMsg::Frame(item) => {
                        match ch.send(proto::envelope::Payload::VideoFrame(
                            arsp::video_frame_to_wire(&item),
                        )).await {
                            Ok(()) => report.record_frame_sent(item.data.len()),
                            Err(e) if conn_gone(&e) => {
                                log.record(AuditEventKind::Disconnected, session_id,
                                    serde_json::json!({"reason":"connection_gone","role":"peer"}))?;
                                emit.emit(ev("closed", serde_json::json!({"reason": "connection_gone"})));
                                break;
                            }
                            Err(e) => return Err(e.into()),
                        }
                    }
                }
            }
            reason = bye_rx.recv(), if !bye_done => {
                let Some(reason) = reason else { bye_done = true; continue; };
                let reason = if reason.trim().is_empty() { "stdin".to_string() } else { reason };
                let _ = ch.send(proto::envelope::Payload::Bye(proto::Bye { reason: reason.clone() })).await;
                tokio::time::sleep(std::time::Duration::from_millis(150)).await; // gracia de cierre
                log.record(AuditEventKind::Disconnected, session_id,
                    serde_json::json!({"reason": reason, "role": "listener"}))?;
                emit.emit(ev("closed", serde_json::json!({"reason": reason})));
                break;
            }
            _ = tokio::signal::ctrl_c() => {
                let _ = ch.send(proto::envelope::Payload::Bye(proto::Bye { reason: "signal".into() })).await;
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                log.record(AuditEventKind::Disconnected, session_id,
                    serde_json::json!({"reason":"signal","role":"listener"}))?;
                emit.emit(ev("closed", serde_json::json!({"reason": "signal"})));
                break;
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
    // La lectura final del portapapeles NO debe romper el cierre (modo flaky/bloqueo real).
    let clipboard_final = match engine.text() {
        Ok(t) => t,
        Err(e) => {
            log.record(AuditEventKind::Error, session_id,
                serde_json::json!({"clipboard": format!("{e:?}"), "dir":"final"}))?;
            emit.emit(ev("clipboard_error", serde_json::json!({"dir":"final","detail": format!("{e:?}")})));
            None
        }
    };
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
    let bye_rx = build_bye_source(&mut cfg);
    let target = cfg
        .target
        .clone()
        .ok_or_else(|| DaemonError::Config("falta el objetivo (o --profile)".into()))?;

    log.record(
        AuditEventKind::SessionStarted,
        &session_id,
        serde_json::json!({"target": target.clone()}),
    )?;
    emit.emit(ev("connecting", serde_json::json!({ "target": target.clone(), "node_id": identity.node_id().to_string(), "daemon": DAEMON_VERSION })));

    // Raíz de confianza: pin local si existe; si no, la recibe por bootstrap del
    // par y se persiste SOLO tras completar la autenticación mutua (C2).
    let mut tcp = TcpStream::connect(&target)
        .await
        .map_err(|e| DaemonError::Io(e.to_string()))?;
    let incoming = anyremote_transport::cert_store::read_bootstrap(&mut tcp).await?;
    let pin_pem = anyremote_transport::cert_store::load_trusted(&cfg.config_dir, &target)?;
    let (root, pending_pin) = match (pin_pem, incoming) {
        (Some(pem), inc) => {
            let root = anyremote_transport::cert_store::cert_from_pem(&pem)?;
            if let Some((_, inc_cert)) = inc {
                let fp_local = anyremote_transport::cert_store::cert_fingerprint(&root);
                let fp_inc = anyremote_transport::cert_store::cert_fingerprint(&inc_cert);
                if fp_inc != fp_local {
                    return Err(DaemonError::Config(format!(
                        "el cert del servidor ({fp_inc}…) no coincide con tu pin de {target} ({fp_local}…) \
                         — posible intermediario. Si el servidor cambió legítimamente, borra {} \
                         y reconecta",
                        anyremote_transport::cert_store::trusted_path(&cfg.config_dir, &target).display()
                    )));
                }
            }
            (root, None)
        }
        (None, Some((pem, cert))) => (cert, Some(pem)),
        (None, None) => {
            return Err(DaemonError::Config(
                "sin pin para este destino y el par no envió cert bootstrap — ¿está escuchando un anyremoted?".into(),
            ))
        }
    };
    let cli_cfg = anyremote_transport::client_config(&root)?;

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
    if let Some(pem) = pending_pin {
        // Auth OK: el pin recibido es legítimo para esta identidad → persistir (por destino).
        anyremote_transport::cert_store::save_trusted(&cfg.config_dir, &target, &pem)?;
        emit.emit(ev("cert_pinned", serde_json::json!({"target": target.clone()})));
    }
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

    match active_controller(&mut ch, &mut log, &mut report, &mut emit, &cfg, &session_id, &peer_id.node_id.to_string(), bye_rx).await {
        Ok(o) => Ok(o),
        Err(e) => {
            // Errores de sesión en curso → cierre limpio auditable.
            log.record(AuditEventKind::Error, &session_id,
                serde_json::json!({"session_error": e.to_string()}))?;
            emit.emit(ev("closed", serde_json::json!({"reason": format!("session_error: {e}")})));
            report.finish();
            log.record(AuditEventKind::Disconnected, &session_id, report.disconnect_event())?;
            Ok(RunOutcome { report, events: emit.events, clipboard_final: None })
        }
    }
}

async fn active_controller(
    ch: &mut arsp::Channel<tokio_rustls::client::TlsStream<TcpStream>>,
    log: &mut SessionLog,
    report: &mut SessionReport,
    emit: &mut Emit,
    cfg: &DaemonConfig,
    session_id: &str,
    peer: &str,
    mut bye_rx: tokio::sync::mpsc::Receiver<String>,
) -> Result<RunOutcome, DaemonError> {
    let mut engine = Engine::new(make_clipboard(cfg)?);
    let mut files = FileReceiver::new_with_store(file_policy(cfg), cfg.config_dir.join("transfers"));
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(cfg.max_secs);
    let mut hb_tick = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut clip_tick = tokio::time::interval(std::time::Duration::from_millis(500));
    let mut stats = LinkStats::default();
    let mut next_seq: u64 = 1;
    let state = SessionState::Active;
    // Resiliencia de portapapeles (v0.1.4): un fallo desactiva el subsistema, no la sesión.
    let mut clip_off = false;
    let mut bye_done = false;

    // Hito M2: preview local de lo que llega del controlado + decodificador con estado.
    let (preview_srv, preview_port, _preview_task) = preview::PreviewServer::start();
    emit.emit(ev("preview", serde_json::json!({ "port": preview_port })));
    let preview = Arc::new(preview_srv);
    let mut decoder: Option<anyremote_codec::NativeDecoder> = None;
    let mut video_frames: u64 = 0;
    let mut video_seq: u32 = 0;

    // C5: envío de archivo opcional tras activar (espera FileReady: offset de reanudación).
    let mut pending_file: Option<(u64, String, String)> = None;
    if let Some(path) = cfg.send_file.clone() {
        let data = std::fs::read(&path).map_err(|e| DaemonError::Io(format!("{path:?}: {e}")))?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("archivo.bin")
            .to_string();
        let sender = FileSender::new(file_policy(cfg));
        let meta = sender.offer(1, &name, &data)?;
        let hex = sha256_hex(&data);
        ch.send(proto::envelope::Payload::FileOffer(arsp::file_offer_to_wire(&meta))).await?;
        emit.emit(ev("file_sent", serde_json::json!({"name": name.clone()})));

        let mut resume_from = 0u64;
        loop {
            let env = ch.recv().await?;
            match env.payload {
                Some(proto::envelope::Payload::FileReady(fr)) if fr.transfer_id == meta.transfer_id => {
                    resume_from = fr.resume_offset;
                    break;
                }
                Some(proto::envelope::Payload::Clipboard(w)) => {
                    let content = arsp::clipboard_from_wire(w)?;
                    let _ = engine.apply_remote(&content);
                }
                Some(proto::envelope::Payload::Heartbeat(_)) => {}
                Some(proto::envelope::Payload::VideoFrame(_)) => {}
                other => {
                    return Err(DaemonError::Config(format!("esperaba FileReady: {other:?}")))
                }
            }
        }
        for (off, chunk) in sender.chunks_from(&data, resume_from, 32 * 1024) {
            ch.send(proto::envelope::Payload::FileChunk(arsp::file_chunk_to_wire(
                meta.transfer_id,
                off,
                chunk,
            )))
            .await?;
        }
        report.record_frame_sent((data.len() as u64 - resume_from) as usize);
        pending_file = Some((meta.transfer_id, hex, name.clone()));
        emit.emit(ev(
            "file_resume",
            serde_json::json!({"from_offset": resume_from, "name": name}),
        ));
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
                        match engine.apply_remote(&content) {
                            Ok(true) => {
                                log.record(AuditEventKind::ClipboardSync, session_id,
                                    serde_json::json!({"dir":"in","bytes": content.text.len()}))?;
                                emit.emit(ev("clipboard_synced", serde_json::json!({"dir":"in"})));
                            }
                            Ok(false) => {}
                            Err(e) => {
                                // Resiliencia: aplicar el portapapeles local falló → desactiva, la sesión sigue.
                                clip_off = true;
                                log.record(AuditEventKind::Error, session_id,
                                    serde_json::json!({"clipboard": format!("{e:?}"), "dir":"in"}))?;
                                emit.emit(ev("clipboard_error", serde_json::json!({"dir":"in","detail": format!("{e:?}")})));
                            }
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
                    Some(proto::envelope::Payload::FileOffer(o)) => {
                        if cfg.one_way_files {
                            // Política de un solo sentido (patrón OPTION_ONE_WAY_FILE_TRANSFER):
                            // solo controlador → controlado; esta oferta entrante se rechaza.
                            let _ = ch.send(proto::envelope::Payload::FileDone(arsp::file_done_to_wire(
                                o.transfer_id,
                                false,
                                "one_way: enlace de un solo sentido (solo controlador→controlado)",
                            ))).await;
                            log.record(AuditEventKind::Error, session_id,
                                serde_json::json!({"file_reject": "one_way"}))?;
                            emit.emit(ev("file_rejected", serde_json::json!({"detail": "one_way"})));
                            continue;
                        }
                        match receive_file_offer(ch, &mut files, o).await {
                            Ok((data, tid)) => {
                                let hex = sha256_hex(&data);
                                let _ = ch.send(proto::envelope::Payload::FileDone(
                                    arsp::file_done_to_wire(tid, true, &hex),
                                )).await;
                                report.record_file();
                                log.record(AuditEventKind::FileTransfer, session_id,
                                    serde_json::json!({"dir":"in","bytes": data.len(), "sha256": hex}))?;
                                emit.emit(ev("file_received", serde_json::json!({"bytes": data.len()})));
                            }
                            Err((tid, detail)) => {
                                let _ = ch.send(proto::envelope::Payload::FileDone(
                                    arsp::file_done_to_wire(tid, false, &detail),
                                )).await;
                                log.record(AuditEventKind::Error, session_id,
                                    serde_json::json!({"file": detail.clone()}))?;
                                emit.emit(ev("file_error", serde_json::json!({"detail": detail})));
                            }
                        }
                    }
                    Some(proto::envelope::Payload::VideoFrame(vf)) => {
                        // Hito M2: decodifica y alimenta el preview local (errores no matan la sesión).
                        match arsp::video_frame_from_wire(vf) {
                            Ok(enc) if !enc.data.is_empty() => {
                                let dec = decoder.get_or_insert_with(anyremote_codec::NativeDecoder::new);
                                match dec.decode(&enc) {
                                    Ok(img) => {
                                        preview.push(img.width, img.height, video_seq, &img.data);
                                        video_seq = video_seq.wrapping_add(1);
                                        video_frames += 1;
                                        report.record_bytes_recv(enc.data.len());
                                        if video_frames % 60 == 1 {
                                            emit.emit(ev("video_stats", serde_json::json!({
                                                "frames": video_frames, "w": img.width, "h": img.height
                                            })));
                                        }
                                    }
                                    Err(e) => {
                                        log.record(AuditEventKind::Error, session_id,
                                            serde_json::json!({"video": format!("{e:?}")}))?;
                                        emit.emit(ev("video_error", serde_json::json!({"detail": format!("{e:?}")})));
                                    }
                                }
                            }
                            Ok(_) => {} // frame omitido por el codificador
                            Err(e) => {
                                emit.emit(ev("video_error", serde_json::json!({"detail": e.to_string()})));
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
                if let Err(e) = ch.send(proto::envelope::Payload::Heartbeat(proto::Heartbeat {
                    seq: next_seq,
                    sent_unix_ms: now,
                    echo: false,
                })).await {
                    if conn_gone(&e) {
                        log.record(AuditEventKind::Disconnected, session_id,
                            serde_json::json!({"reason":"connection_gone","role":"peer"}))?;
                        emit.emit(ev("closed", serde_json::json!({"reason": "connection_gone"})));
                        break;
                    }
                    return Err(e.into());
                }
                next_seq += 1;
            }
            _ = clip_tick.tick(), if cfg.clipboard != ClipboardMode::Off && !clip_off => {
                match engine.detect_local_change() {
                    Ok(Some(content)) => {
                        match arsp::clipboard_to_wire(&content) {
                            Ok(w) => match ch.send(proto::envelope::Payload::Clipboard(w)).await {
                                Ok(()) => {
                                    log.record(AuditEventKind::ClipboardSync, session_id,
                                        serde_json::json!({"dir":"out","bytes": content.text.len()}))?;
                                    emit.emit(ev("clipboard_synced", serde_json::json!({"dir":"out"})));
                                }
                                Err(e) if conn_gone(&e) => {
                                    log.record(AuditEventKind::Disconnected, session_id,
                                        serde_json::json!({"reason":"connection_gone","role":"peer"}))?;
                                    emit.emit(ev("closed", serde_json::json!({"reason": "connection_gone"})));
                                    break;
                                }
                                Err(e) => {
                                    clip_off = true;
                                    log.record(AuditEventKind::Error, session_id,
                                        serde_json::json!({"clipboard": format!("{e:?}"), "dir":"out"}))?;
                                    emit.emit(ev("clipboard_error", serde_json::json!({"dir":"out","detail": format!("{e:?}")})));
                                }
                            },
                            Err(e) => {
                                clip_off = true;
                                log.record(AuditEventKind::Error, session_id,
                                    serde_json::json!({"clipboard": format!("{e:?}"), "dir":"out"}))?;
                                emit.emit(ev("clipboard_error", serde_json::json!({"dir":"out","detail": format!("{e:?}")})));
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        // Resiliencia: lectura local falló → desactiva portapapeles, la sesión sigue.
                        clip_off = true;
                        log.record(AuditEventKind::Error, session_id,
                            serde_json::json!({"clipboard": format!("{e:?}"), "dir":"out"}))?;
                        emit.emit(ev("clipboard_error", serde_json::json!({"dir":"out","detail": format!("{e:?}")})));
                    }
                }
            }
            reason = bye_rx.recv(), if !bye_done => {
                let Some(reason) = reason else { bye_done = true; continue; };
                let reason = if reason.trim().is_empty() { "stdin".to_string() } else { reason };
                let _ = ch.send(proto::envelope::Payload::Bye(proto::Bye { reason: reason.clone() })).await;
                tokio::time::sleep(std::time::Duration::from_millis(150)).await; // gracia de cierre
                log.record(AuditEventKind::Disconnected, session_id,
                    serde_json::json!({"reason": reason, "role": "controller"}))?;
                emit.emit(ev("closed", serde_json::json!({"reason": reason})));
                break;
            }
            _ = tokio::signal::ctrl_c() => {
                let _ = ch.send(proto::envelope::Payload::Bye(proto::Bye { reason: "signal".into() })).await;
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                log.record(AuditEventKind::Disconnected, session_id,
                    serde_json::json!({"reason":"signal","role":"controller"}))?;
                emit.emit(ev("closed", serde_json::json!({"reason": "signal"})));
                break;
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
    // La lectura final del portapapeles NO debe romper el cierre (modo flaky/bloqueo real).
    let clipboard_final = match engine.text() {
        Ok(t) => t,
        Err(e) => {
            log.record(AuditEventKind::Error, session_id,
                serde_json::json!({"clipboard": format!("{e:?}"), "dir":"final"}))?;
            emit.emit(ev("clipboard_error", serde_json::json!({"dir":"final","detail": format!("{e:?}")})));
            None
        }
    };
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
