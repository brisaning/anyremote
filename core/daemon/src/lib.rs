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

use anyremote_audio::{AudioCapture, AudioMode, AudioPlayback, AudioSource, OpusCodec};
use anyremote_clipboard::{ClipboardSyncEngine, MemoryClipboard};
use anyremote_codec::VideoDecoder;
use anyremote_filexfer::{sha256_hex, FileReceiver, FileSender};
use anyremote_identity::{Identity, TrustStore};
use anyremote_input::InputInjector;
use anyremote_session::{LinkStats, SessionEvent, SessionState};
use anyremote_telemetry::{AuditEventKind, SessionLog, SessionReport};
use anyremote_transport::arsp::{self, proto};
use anyremote_transport::auth;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::{TlsAcceptor, TlsConnector};

pub mod preview;
pub mod video;

use video::{spawn_video_task, VideoMode, VideoMsg};

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
    /// RF-02a (D-07): permite inyectar teclado/ratón en el controlado.
    /// **Desactivado por defecto** (denegado salvo que se pida `--allow-input`).
    pub allow_input: bool,
    /// Backend de inyección: `Windows` (SendInput) en producción, `Recording` en tests.
    pub input_backend: InputBackend,
    /// RF audio (D-07): modo de audio de sesión — `listen`: fuente de captura,
    /// `connect`: reproductor. `Off` por defecto; `Test` sin hardware (gates).
    pub audio: AudioMode,
    /// QW-3: tope de ancho de banda del video en bps (`--max-bitrate`, en kbps).
    /// Sin tope, el adaptativo puede rampar hasta `bps*4`.
    pub video_max_bps: Option<u64>,
    /// Fuente de captura en `listen`: `System` por defecto (loopback: lo que
    /// suena en los altavoces); `--audio-source mic|system,mic` para cambiarlo.
    /// Con varias fuentes se mezclan (C-2).
    pub audio_sources: Vec<AudioSource>,
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
            allow_input: false,
            input_backend: InputBackend::Windows,
            audio: AudioMode::Off,
            video_max_bps: None,
            audio_sources: vec![AudioSource::System],
            bye_source: None,
        }
    }
}

/// Backend de inyección de entrada (RF-02a): `Windows` en producción;
/// `Recording` en tests (registra sin tocar el sistema, patrón H-RD-06).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputBackend {
    Recording,
    Windows,
}

/// Construye el inyector según el backend configurado.
fn make_injector(backend: InputBackend) -> anyremote_input::PlatformInjector {
    use anyremote_input::{PlatformInjector, RecordingInjector};
    match backend {
        InputBackend::Recording => PlatformInjector::Recording(RecordingInjector::new()),
        #[cfg(windows)]
        InputBackend::Windows => {
            PlatformInjector::Windows(anyremote_input::windows_backend::SendInputInjector::new())
        }
        // El producto es Windows; fuera de él solo existe el backend de registro
        // para que el workspace compile (no inyección real).
        #[cfg(not(windows))]
        InputBackend::Windows => PlatformInjector::Recording(RecordingInjector::new()),
    }
}

/// Permiso de sesión que exige cada evento (D-07): teclado vs ratón.
fn input_permission(ev: &anyremote_input::InputEvent) -> &'static str {
    use anyremote_input::InputEvent;
    match ev {
        InputEvent::KeyDown(_) | InputEvent::KeyUp(_) => "keyboard",
        _ => "mouse",
    }
}

/// Contrato de stdin del controlador (app → daemon): líneas `{"input":{...}}`
/// (RF-02a). `Ok(None)` = JSON que no es un evento de entrada; `Err` = detalle
/// para auditar. La validación reutiliza el mapeo owner del wire (transport).
fn parse_input_line(line: &str) -> Result<Option<proto::InputEventMsg>, String> {
    let v: serde_json::Value = serde_json::from_str(line).map_err(|e| format!("json: {e}"))?;
    let Some(spec) = v.get("input") else {
        return Ok(None);
    };
    let msg = if let Some(k) = spec.get("key") {
        proto::InputEventMsg {
            ev: Some(proto::input_event_msg::Ev::Key(proto::Key {
                code: k.get("code").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                down: k.get("down").and_then(|x| x.as_bool()).unwrap_or(false),
                extended: false,
                ch: k
                    .get("ch")
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .to_string(),
            })),
        }
    } else if let Some(p) = spec.get("pointer") {
        proto::InputEventMsg {
            ev: Some(proto::input_event_msg::Ev::Pointer(proto::Pointer {
                x: p.get("x").and_then(|x| x.as_i64()).unwrap_or(0) as i32,
                y: p.get("y").and_then(|x| x.as_i64()).unwrap_or(0) as i32,
                absolute: p.get("absolute").and_then(|x| x.as_bool()).unwrap_or(true),
            })),
        }
    } else if let Some(b) = spec.get("button") {
        proto::InputEventMsg {
            ev: Some(proto::input_event_msg::Ev::Button(proto::Button {
                button: b.get("button").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                down: b.get("down").and_then(|x| x.as_bool()).unwrap_or(false),
                wheel: b.get("wheel").and_then(|x| x.as_i64()).unwrap_or(0) as i32,
            })),
        }
    } else {
        return Err("input sin key/pointer/button".into());
    };
    // Rechaza aquí mismo lo que el receptor no sabría inyectar (código/caracter inválido).
    if arsp::input_event_from_wire(&msg).is_none() {
        return Err("evento de entrada no soportado (código o carácter)".into());
    }
    Ok(Some(msg))
}

/// Comando de permisos DINÁMICOS del anfitrión por stdin (C-2):
/// `{"perm":{"input":bool,"audio":bool,"mic":bool}}` — los tres campos viajan
/// siempre (los ausentes valen `false`). `Ok(None)` = no es un comando de
/// permisos; `Err` = detalle para auditar.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct PermSpec {
    input: bool,
    audio: bool,
    mic: bool,
}

fn parse_perm_line(line: &str) -> Result<Option<PermSpec>, String> {
    let v: serde_json::Value = serde_json::from_str(line).map_err(|e| format!("json: {e}"))?;
    let Some(p) = v.get("perm") else {
        return Ok(None);
    };
    Ok(Some(PermSpec {
        input: p.get("input").and_then(|x| x.as_bool()).unwrap_or(false),
        audio: p.get("audio").and_then(|x| x.as_bool()).unwrap_or(false),
        mic: p.get("mic").and_then(|x| x.as_bool()).unwrap_or(false),
    }))
}

/// QW-2: umbral de silencio (RMS normalizado) y frames a omitir tras el.
const SILENCE_RMS: f64 = 0.002;
const SILENCE_SKIP_FRAMES: u32 = 100; // 2 s de frames a 20 ms

/// QW-2: gate de silencio - `true` = omitir este frame (silencio sostenido):
/// no se codifica ni se envia audio cuando no hay nada que oir.
fn silence_gate(rms: f64, silent_frames: &mut u32) -> bool {
    if rms < SILENCE_RMS {
        *silent_frames += 1;
        *silent_frames > SILENCE_SKIP_FRAMES
    } else {
        *silent_frames = 0;
        false
    }
}

/// Arranca la captura de audio con las fuentes dadas; traza `audio_start` (y
/// `audio_error` por fuente no disponible — la captura degrada, no falla).
fn spawn_audio_capture(
    mode: AudioMode,
    sources: &[AudioSource],
    session_id: &str,
    log: &mut SessionLog,
    emit: &mut Emit,
) -> Option<(AudioCapture, mpsc::Receiver<Vec<i16>>)> {
    let (tx, rx) = mpsc::channel::<Vec<i16>>(16);
    match AudioCapture::start(mode, sources, tx) {
        Ok(cap) => {
            for w in cap.warnings() {
                // Un fallo de auditoría no debe bloquear el audio (resiliencia).
                let _ = log.record(
                    AuditEventKind::Error,
                    session_id,
                    serde_json::json!({"audio": w}),
                );
                emit.emit(ev("audio_error", serde_json::json!({"detail": w})));
            }
            emit.emit(ev(
                "audio_start",
                serde_json::json!({
                    "mode": format!("{mode:?}"),
                    "source": sources.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(","),
                    "rate": anyremote_audio::AUDIO_RATE,
                }),
            ));
            Some((cap, rx))
        }
        Err(e) => {
            let _ = log.record(
                AuditEventKind::Error,
                session_id,
                serde_json::json!({"audio": e.to_string()}),
            );
            emit.emit(ev(
                "audio_error",
                serde_json::json!({"detail": e.to_string()}),
            ));
            None
        }
    }
}

/// Estado dinámico de permisos del anfitrión (C-2): toggles en caliente que la
/// UI envía por stdin durante la sesión. El techo D-07 sigue siendo la
/// concesión del handshake (`granted`) AND los flags de arranque (capacidad);
/// la excepción es audio: compartir tu audio/micrófono es decisión de quien
/// comparte, así que activarlo en caliente no exige el grant inicial.
struct DynamicPerms {
    /// Toggle de entrada (siempre ANDado con `--allow-input` y el grant).
    input_on: bool,
    /// Fuentes de audio activas (vacío = sin captura).
    sources: Vec<AudioSource>,
    /// El anfitrión activó audio en caliente → sin exigir el grant inicial.
    runtime_enabled: bool,
    granted_audio: bool,
    /// C-5: el estado de captura YA se intentó (un solo `spawn` por sesión:
    /// si falló aquí no se reintenta en el arranque inicial → un único
    /// `audio_error` y sin segundos intentos ambiguos).
    spawn_attempted: bool,
}

impl DynamicPerms {
    fn new(cfg: &DaemonConfig, granted: &[String]) -> Self {
        let granted_audio = granted.iter().any(|p| p == "audio");
        let sources = if cfg.audio != AudioMode::Off && granted_audio {
            cfg.audio_sources.clone()
        } else {
            Vec::new()
        };
        Self {
            input_on: cfg.allow_input,
            sources,
            runtime_enabled: false,
            granted_audio,
            spawn_attempted: false,
        }
    }

    /// Aplica un comando de la UI y (re)arranca o detiene la captura.
    #[allow(clippy::too_many_arguments)]
    fn apply(
        &mut self,
        perm: PermSpec,
        cfg: &DaemonConfig,
        session_id: &str,
        capture: &mut Option<AudioCapture>,
        pcm_rx: &mut Option<mpsc::Receiver<Vec<i16>>>,
        log: &mut SessionLog,
        emit: &mut Emit,
    ) {
        self.input_on = cfg.allow_input && perm.input;
        let mut wanted = Vec::new();
        if perm.audio {
            wanted.push(AudioSource::System);
        }
        if perm.mic {
            wanted.push(AudioSource::Microphone);
        }
        if !wanted.is_empty() {
            self.runtime_enabled = true;
        }
        let allowed = cfg.audio != AudioMode::Off
            && !wanted.is_empty()
            && (self.granted_audio || self.runtime_enabled);
        let wanted: Vec<AudioSource> = if allowed { wanted } else { Vec::new() };
        if wanted != self.sources {
            // Detener la captura actual (Drop une el hilo) y arrancar la nueva.
            self.spawn_attempted = true;
            *capture = None;
            *pcm_rx = None;
            if wanted.is_empty() {
                emit.emit(ev(
                    "audio_stopped",
                    serde_json::json!({"reason": "permisos"}),
                ));
            } else if let Some((cap, rx)) =
                spawn_audio_capture(cfg.audio, &wanted, session_id, log, emit)
            {
                *capture = Some(cap);
                *pcm_rx = Some(rx);
            }
            self.sources = wanted;
        }
        emit.emit(ev(
            "permissions_updated",
            serde_json::json!({
                "input": self.input_on,
                "audio": self.sources.contains(&AudioSource::System),
                "mic": self.sources.contains(&AudioSource::Microphone),
            }),
        ));
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
/// El buffer es grande porque el visor escribe también líneas `{"input":...}` (RF-02a).
fn build_bye_source(cfg: &mut DaemonConfig) -> tokio::sync::mpsc::Receiver<String> {
    if let Some(rx) = cfg.bye_source.take() {
        return rx;
    }
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(64);
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
                Err(DaemonError::Config(
                    "--clipboard win no soportado aquí".into(),
                ))
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
    [
        "10053", "10054", "1006", "eof", "anulada", "reset", "closed",
    ]
    .iter()
    .any(|k| s.contains(k))
}

/// Resultado del establecimiento con un cliente.
#[allow(clippy::large_enum_variant)] // `Channel` domina en tamaño; boxear solo añadiría indirección.
enum Established {
    /// Sesión autenticada y con permisos → `active_controlled` con la
    /// **concesión efectiva** (`granted`, D-07) para validar cada evento de entrada.
    Active(
        arsp::Channel<tokio_rustls::server::TlsStream<TcpStream>>,
        Vec<String>,
    ),
    /// Permiso denegado: cliente descartado; el listener sigue vivo.
    Denied,
}

/// Handshake con UN cliente ya aceptado: bootstrap → TLS → auth (C2) → permisos (C1).
/// Todo fallo aquí es del cliente (el caller continúa aceptando).
#[allow(clippy::too_many_arguments)] // capas de sesión: cada parámetro tiene dueño claro.
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
    let trust = TrustStore::load(&cfg.config_dir);
    let peer_id = auth::recv_validate_auth(&mut ch, &mut gate, &trust, &binding).await?;
    auth::send_auth(&mut ch, identity, &binding).await?;
    // D-03 (TOFU): se recuerda el par autenticado; en la próxima sesión del
    // mismo equipo `peer_trusted` será true.
    TrustStore::remember(&cfg.config_dir, &peer_id.node_id, &peer_id.pubkey);
    log.record(
        AuditEventKind::SessionEstablished,
        session_id,
        serde_json::json!({"peer": peer_id.node_id.to_string(), "peer_trusted": peer_id.trusted, "peer_addr": peer.to_string()}),
    )?;
    emit.emit(ev(
        "session_active",
        serde_json::json!({"role": "controlled", "peer": peer_id.node_id.to_string(), "session_id": session_id, "peer_trusted": peer_id.trusted}),
    ));

    // C1: hello/welcome + permisos (el controlado SOLICITA; D-07 por defecto nada activo).
    let hello = ch.recv().await?;
    arsp::check_version(&hello).map_err(DaemonError::Config)?;
    ch.send(arsp::welcome(&identity.node_id().to_string()))
        .await?;
    let mut requested = cfg.grant.clone();
    if cfg.audio != AudioMode::Off && !requested.iter().any(|p| p == "audio") {
        requested.push("audio".into()); // RF audio: pedir `audio` solo con `--audio`
    }
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
        emit.emit(ev(
            "client_failed",
            serde_json::json!({"peer": peer.to_string(), "detail": "permission_denied"}),
        ));
        return Ok(Established::Denied);
    }
    log.record(
        AuditEventKind::PermissionGranted,
        session_id,
        serde_json::json!({"perms": granted.permissions}),
    )?;
    Ok(Established::Active(ch, granted.permissions))
}

/// Lado controlado: acepta sesiones (`anyremoted listen`).
///
/// **Resiliencia (prueba cross-device)**: un cliente que falle en cualquier etapa
/// (TLS, auth, secuencia, silencio >15 s) se audita como `client_failed` y el
/// listener **sigue aceptando**; solo termina con una sesión completada o al
/// agotar `--max-secs` de vida del listener.
pub async fn run_listen(mut cfg: DaemonConfig) -> Result<RunOutcome, DaemonError> {
    // D-03 (v0.8.0): identidad persistida → el NodeId es estable por equipo.
    let identity = Identity::load_or_generate(&cfg.config_dir)
        .map_err(|e| DaemonError::Config(format!("identidad: {e:?}")))?;
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
    let listen_deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(cfg.max_secs);

    loop {
        let remaining = listen_deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            log.record(
                AuditEventKind::Disconnected,
                &session_id,
                serde_json::json!({"reason":"listen_timeout","role":"listener"}),
            )?;
            emit.emit(ev(
                "closed",
                serde_json::json!({"reason": "listen_timeout"}),
            ));
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
                emit.emit(ev(
                    "closed",
                    serde_json::json!({"reason": "listen_timeout"}),
                ));
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
            Ok(Ok(Established::Active(mut ch, granted))) => {
                // Errores de sesión en curso → cierre limpio auditable (no matan el listener como proceso con pánico).
                return match active_controlled(
                    &mut ch,
                    &mut log,
                    &mut report,
                    &mut emit,
                    &cfg,
                    &session_id,
                    &granted,
                    bye_rx,
                )
                .await
                {
                    Ok(o) => Ok(o),
                    Err(e) => {
                        log.record(
                            AuditEventKind::Error,
                            &session_id,
                            serde_json::json!({"session_error": e.to_string()}),
                        )?;
                        emit.emit(ev(
                            "closed",
                            serde_json::json!({"reason": format!("session_error: {e}")}),
                        ));
                        report.finish();
                        log.record(
                            AuditEventKind::Disconnected,
                            &session_id,
                            report.disconnect_event(),
                        )?;
                        Ok(RunOutcome {
                            report,
                            events: emit.events,
                            clipboard_final: None,
                        })
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

#[allow(clippy::too_many_arguments)] // capas de sesión con dueño claro por parámetro.
async fn active_controlled(
    ch: &mut arsp::Channel<tokio_rustls::server::TlsStream<TcpStream>>,
    log: &mut SessionLog,
    report: &mut SessionReport,
    emit: &mut Emit,
    cfg: &DaemonConfig,
    session_id: &str,
    granted: &[String],
    mut bye_rx: tokio::sync::mpsc::Receiver<String>,
) -> Result<RunOutcome, DaemonError> {
    let mut engine = Engine::new(make_clipboard(cfg)?);
    let mut files =
        FileReceiver::new_with_store(file_policy(cfg), cfg.config_dir.join("transfers"));
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(cfg.max_secs);
    let mut clip_tick = tokio::time::interval(std::time::Duration::from_millis(500));
    let state = SessionState::Active;
    // Un fallo de portapapeles NO mata la sesión: se desactiva el subsistema (v0.1.4).
    let mut clip_off = false;
    // RF-02a (D-07): inyector siempre disponible; lo gobierna el toggle
    // dinámico `perms.input_on` ANDado con `--allow-input` y la concesión.
    let mut injector = make_injector(cfg.input_backend);
    // RF audio (D-07): codec + estado dinámico de permisos del anfitrión (C-2).
    let mut codec = (cfg.audio != AudioMode::Off)
        .then(OpusCodec::new)
        .transpose()
        .map_err(|e| DaemonError::Config(e.to_string()))?;
    let mut perms = DynamicPerms::new(cfg, granted);
    let mut audio_capture: Option<AudioCapture> = None;
    let mut pcm_rx: Option<mpsc::Receiver<Vec<i16>>> = None;
    if cfg.audio != AudioMode::Off && !perms.granted_audio {
        emit.emit(ev(
            "audio_denied",
            serde_json::json!({"reason": "no_concedido"}),
        ));
    }
    // Estado inicial dinámico: la UI escribe `{"perm":...}` al arrancar el
    // listener; se aplica ANTES del primer evento de la sesión (sin carrera
    // con la primera entrada que llegue del controlador).
    let mut pending_bye: Option<String> = None;
    while let Ok(line) = bye_rx.try_recv() {
        match parse_perm_line(&line) {
            Ok(Some(perm)) => {
                perms.apply(
                    perm,
                    cfg,
                    session_id,
                    &mut audio_capture,
                    &mut pcm_rx,
                    log,
                    emit,
                );
            }
            _ => {
                if pending_bye.is_none() {
                    pending_bye = Some(line);
                }
            }
        }
    }
    // QW-2: contador de silencio sostenido (gate de envio de audio).
    let mut silent_frames: u32 = 0;
    // Captura inicial si ningún comando la decidió ya (nunca si el comando en
    // caliente ya decidió el estado → un solo intento por sesión, C-5).
    if !perms.spawn_attempted && audio_capture.is_none() && !perms.sources.is_empty() {
        if let Some((cap, rx)) =
            spawn_audio_capture(cfg.audio, &perms.sources, session_id, log, emit)
        {
            audio_capture = Some(cap);
            pcm_rx = Some(rx);
        }
    }

    // Hito M2: vídeo hacia el canal + preview local de la propia pantalla (solo listen).
    let (video_tx, mut video_rx) = mpsc::channel::<VideoMsg>(3);
    let stop = Arc::new(AtomicBool::new(false));
    let _stop_guard = StopGuard(stop.clone());
    let mut video_done = false;
    let mut bye_done = false;
    if cfg.video != VideoMode::Off {
        let (preview_srv, preview_port, _preview_task) = preview::PreviewServer::start();
        emit.emit(ev("preview", serde_json::json!({ "port": preview_port })));
        emit.emit(ev(
            "video_start",
            serde_json::json!({
                "mode": format!("{:?}", cfg.video),
                "width": cfg.video_size.0, "height": cfg.video_size.1,
                "fps": cfg.video_fps, "codec": format!("{:?}", cfg.codec),
            }),
        ));
        let _video_thread = spawn_video_task(
            cfg.video,
            cfg.video_size,
            cfg.video_fps,
            cfg.codec,
            cfg.video_max_bps.map(|b| b as usize),
            video_tx,
            preview_srv,
            stop,
        );
    }

    loop {
        // Cierre encolado antes de la sesión (p. ej. `bye` escrito al arrancar
        // el listener): se cumple al iniciar ésta.
        if let Some(reason) = pending_bye.take() {
            let reason = if reason.trim().is_empty() {
                "stdin".to_string()
            } else {
                reason
            };
            let _ = ch
                .send(proto::envelope::Payload::Bye(proto::Bye {
                    reason: reason.clone(),
                }))
                .await;
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            log.record(
                AuditEventKind::Disconnected,
                session_id,
                serde_json::json!({"reason": reason, "role": "listener"}),
            )?;
            emit.emit(ev("closed", serde_json::json!({"reason": reason})));
            break;
        }
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
                    Some(proto::envelope::Payload::AudioFrame(_)) => {
                        // El controlado no recibe audio: ignora con seguridad.
                    }
                    Some(proto::envelope::Payload::InputEvent(m)) => {
                        // RF-02a: entrada del controlador → validar permiso (D-07) →
                        // inyectar y ACUSAR al controlador (`InputAck`): el visor
                        // cuenta las realmente inyectadas y ve los rechazos.
                        let (ack_ok, ack_perm, ack_detail) =
                            match arsp::input_event_from_wire(&m) {
                                None => {
                                    log.record(AuditEventKind::InputDenied, session_id,
                                        serde_json::json!({"reason": "unsupported"}))?;
                                    emit.emit(ev("input_denied",
                                        serde_json::json!({"reason": "unsupported"})));
                                    (false, String::new(), "unsupported".to_string())
                                }
                                Some(iev) => {
                                    let need = input_permission(&iev);
                                    let allowed =
                                        perms.input_on && granted.iter().any(|p| p == need);
                                    if !allowed {
                                        let reason = if !perms.input_on {
                                            "toggle_apagado"
                                        } else {
                                            "no_concedido"
                                        };
                                        log.record(AuditEventKind::InputDenied, session_id,
                                            serde_json::json!({"perm": need, "allow_input": perms.input_on}))?;
                                        emit.emit(ev("input_denied",
                                            serde_json::json!({"perm": need, "allow_input": perms.input_on})));
                                        (false, need.to_string(), reason.to_string())
                                    } else {
                                        match injector.inject(&iev) {
                                            Ok(()) => {
                                                let detail = serde_json::json!({"perm": need, "input": format!("{iev:?}")});
                                                log.record(AuditEventKind::InputInjected, session_id, detail.clone())?;
                                                emit.emit(ev("input_injected", detail));
                                                (true, need.to_string(), String::new())
                                            }
                                            Err(e) => {
                                                log.record(AuditEventKind::Error, session_id,
                                                    serde_json::json!({"input": format!("{e:?}")}))?;
                                                emit.emit(ev("input_error",
                                                    serde_json::json!({"detail": format!("{e:?}")})));
                                                (false, need.to_string(), format!("{e:?}"))
                                            }
                                        }
                                    }
                                }
                            };
                        if let Err(e) = ch
                            .send(proto::envelope::Payload::InputAck(proto::InputAck {
                                ok: ack_ok,
                                perm: ack_perm,
                                detail: ack_detail,
                            }))
                            .await
                        {
                            if conn_gone(&e) {
                                log.record(AuditEventKind::Disconnected, session_id,
                                    serde_json::json!({"reason":"connection_gone","role":"peer"}))?;
                                emit.emit(ev("closed", serde_json::json!({"reason": "connection_gone"})));
                                break;
                            }
                            return Err(e.into());
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
            pcm = async {
                match pcm_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending::<Option<Vec<i16>>>().await,
                }
            }, if audio_capture.is_some() => {
                // RF audio: captura → Opus → wire (20 ms, 48 kHz mono).
                let Some(pcm) = pcm else { audio_capture = None; continue; };
                let Some(codec) = codec.as_mut() else { continue; };
                // QW-2: silencio sostenido -> no se codifica ni se envia.
                if silence_gate(anyremote_audio::rms_i16(&pcm), &mut silent_frames) {
                    continue;
                }
                match codec.encode(&pcm) {
                    Ok(frame) => {
                        match ch.send(proto::envelope::Payload::AudioFrame(
                            arsp::audio_frame_to_wire(&frame),
                        )).await {
                            Ok(()) => report.record_audio_sent(frame.data.len()),
                            Err(e) if conn_gone(&e) => {
                                log.record(AuditEventKind::Disconnected, session_id,
                                    serde_json::json!({"reason":"connection_gone","role":"peer"}))?;
                                emit.emit(ev("closed", serde_json::json!({"reason": "connection_gone"})));
                                break;
                            }
                            Err(e) => return Err(e.into()),
                        }
                    }
                    Err(e) => {
                        log.record(AuditEventKind::Error, session_id,
                            serde_json::json!({"audio": e.to_string()}))?;
                        emit.emit(ev("audio_error", serde_json::json!({"detail": e.to_string()})));
                    }
                }
            }
            line = bye_rx.recv(), if !bye_done => {
                let Some(line) = line else { bye_done = true; continue; };
                // C-2: comandos dinámicos de permisos del anfitrión (stdin de la
                // UI) → aplicados en caliente sin reconectar.
                if line.trim_start().starts_with('{') {
                    match parse_perm_line(&line) {
                        Ok(Some(perm)) => {
                            perms.apply(perm, cfg, session_id, &mut audio_capture, &mut pcm_rx,
                                log, emit);
                            continue;
                        }
                        Ok(None) => {} // JSON sin `perm` → se trata como cierre (compat.)
                        Err(detail) => {
                            emit.emit(ev("input_error", serde_json::json!({"detail": detail})));
                            continue;
                        }
                    }
                }
                let reason = if line.trim().is_empty() { "stdin".to_string() } else { line };
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
    log.record(
        AuditEventKind::Disconnected,
        session_id,
        report.disconnect_event(),
    )?;
    // La lectura final del portapapeles NO debe romper el cierre (modo flaky/bloqueo real).
    let clipboard_final = match engine.text() {
        Ok(t) => t,
        Err(e) => {
            log.record(
                AuditEventKind::Error,
                session_id,
                serde_json::json!({"clipboard": format!("{e:?}"), "dir":"final"}),
            )?;
            emit.emit(ev(
                "clipboard_error",
                serde_json::json!({"dir":"final","detail": format!("{e:?}")}),
            ));
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
    // D-03 (v0.8.0): identidad persistida → el NodeId es estable por equipo.
    let identity = Identity::load_or_generate(&cfg.config_dir)
        .map_err(|e| DaemonError::Config(format!("identidad: {e:?}")))?;
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
    // RF audio: ofrecer `audio` si está activo (D-07: la intersección con la
    // solicitud del controlado decide la concesión final).
    if cfg.audio != AudioMode::Off && !cfg.grant.iter().any(|p| p == "audio") {
        cfg.grant.push("audio".into());
    }

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
    let trust = TrustStore::load(&cfg.config_dir);
    let peer_id = auth::recv_validate_auth(&mut ch, &mut gate, &trust, &binding).await?;
    // D-03 (TOFU): recuerda el anfitrión autenticado (además del pin de cert).
    TrustStore::remember(&cfg.config_dir, &peer_id.node_id, &peer_id.pubkey);
    if let Some(pem) = pending_pin {
        // Auth OK: el pin recibido es legítimo para esta identidad → persistir (por destino).
        anyremote_transport::cert_store::save_trusted(&cfg.config_dir, &target, &pem)?;
        emit.emit(ev(
            "cert_pinned",
            serde_json::json!({"target": target.clone()}),
        ));
    }
    log.record(
        AuditEventKind::SessionEstablished,
        &session_id,
        serde_json::json!({"peer": peer_id.node_id.to_string(), "peer_trusted": peer_id.trusted}),
    )?;
    emit.emit(ev(
        "session_active",
        serde_json::json!({"role": "controller", "peer": peer_id.node_id.to_string(), "session_id": session_id, "peer_trusted": peer_id.trusted}),
    ));

    // C1: hello → welcome → solicitud → concesión (intersección con --grant).
    ch.send(arsp::hello(&identity.node_id().to_string(), &session_id))
        .await?;
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
        emit.emit(ev(
            "closed",
            serde_json::json!({"reason": "permission_denied"}),
        ));
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
        serde_json::json!({"perms": allowed}),
    )?;
    // RF audio (C-2): el reproductor queda PREPARADO aunque el anfitrión no
    // comparta al inicio — si lo activa en caliente durante la sesión, sonará.
    // El `audio_denied` inicial es informativo (no llegan frames hasta que comparte).
    if cfg.audio != AudioMode::Off && !allowed.iter().any(|p| p == "audio") {
        emit.emit(ev(
            "audio_denied",
            serde_json::json!({
                "reason": "no_concedido_inicial",
                "detail": "si el anfitrión comparte durante la sesión se reproducirá",
            }),
        ));
    }

    match active_controller(
        &mut ch,
        &mut log,
        &mut report,
        &mut emit,
        &cfg,
        &session_id,
        &peer_id.node_id.to_string(),
        bye_rx,
    )
    .await
    {
        Ok(o) => Ok(o),
        Err(e) => {
            // Errores de sesión en curso → cierre limpio auditable.
            log.record(
                AuditEventKind::Error,
                &session_id,
                serde_json::json!({"session_error": e.to_string()}),
            )?;
            emit.emit(ev(
                "closed",
                serde_json::json!({"reason": format!("session_error: {e}")}),
            ));
            report.finish();
            log.record(
                AuditEventKind::Disconnected,
                &session_id,
                report.disconnect_event(),
            )?;
            Ok(RunOutcome {
                report,
                events: emit.events,
                clipboard_final: None,
            })
        }
    }
}

#[allow(clippy::too_many_arguments)] // capas de sesión con dueño claro por parámetro.
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
    let mut files =
        FileReceiver::new_with_store(file_policy(cfg), cfg.config_dir.join("transfers"));
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(cfg.max_secs);
    let mut hb_tick = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut clip_tick = tokio::time::interval(std::time::Duration::from_millis(500));
    let mut stats = LinkStats::default();
    let mut next_seq: u64 = 1;
    let state = SessionState::Active;
    // Resiliencia de portapapeles (v0.1.4): un fallo desactiva el subsistema, no la sesión.
    let mut clip_off = false;
    let mut bye_done = false;
    // RF audio: reproductor solo con `--audio` (la concesión ya resolvió run_connect).
    let mut codec = (cfg.audio != AudioMode::Off)
        .then(OpusCodec::new)
        .transpose()
        .map_err(|e| DaemonError::Config(e.to_string()))?;
    let mut playback: Option<AudioPlayback> = None;
    if cfg.audio != AudioMode::Off {
        match AudioPlayback::start(cfg.audio) {
            Ok(pb) => {
                emit.emit(ev(
                    "audio_start",
                    serde_json::json!({
                        "mode": format!("{:?}", cfg.audio),
                        "rate": anyremote_audio::AUDIO_RATE,
                    }),
                ));
                playback = Some(pb);
            }
            Err(e) => {
                log.record(
                    AuditEventKind::Error,
                    session_id,
                    serde_json::json!({"audio": e.to_string()}),
                )?;
                emit.emit(ev(
                    "audio_error",
                    serde_json::json!({"detail": e.to_string()}),
                ));
            }
        }
    }
    let mut audio_frames: u64 = 0;
    // QW-2: si el anfitrión deja de enviar (silencio), el visor debe ver
    // "sin senal" -> se emite `audio_stats` con rms 0 cuando no hay frames nuevos.
    let mut audio_tick = tokio::time::interval(std::time::Duration::from_secs(2));
    let mut stats_frames_mark: u64 = 0;
    // Diagnóstico: RMS medio de lo decodificado desde el último `audio_stats`
    // (0.0000 = silencio → "no llega audio real"; >0 = hay sonido).
    let mut rms_acc = 0f64;
    let mut rms_n = 0u32;

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
        ch.send(proto::envelope::Payload::FileOffer(
            arsp::file_offer_to_wire(&meta),
        ))
        .await?;
        emit.emit(ev("file_sent", serde_json::json!({"name": name.clone()})));

        let resume_from = loop {
            let env = ch.recv().await?;
            match env.payload {
                Some(proto::envelope::Payload::FileReady(fr))
                    if fr.transfer_id == meta.transfer_id =>
                {
                    break fr.resume_offset;
                }
                Some(proto::envelope::Payload::Clipboard(w)) => {
                    let content = arsp::clipboard_from_wire(w)?;
                    let _ = engine.apply_remote(&content);
                }
                Some(proto::envelope::Payload::Heartbeat(_)) => {}
                Some(proto::envelope::Payload::VideoFrame(_)) => {}
                other => {
                    return Err(DaemonError::Config(format!(
                        "esperaba FileReady: {other:?}"
                    )))
                }
            }
        };
        for (off, chunk) in sender.chunks_from(&data, resume_from, 32 * 1024) {
            ch.send(proto::envelope::Payload::FileChunk(
                arsp::file_chunk_to_wire(meta.transfer_id, off, chunk),
            ))
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
                    Some(proto::envelope::Payload::AudioFrame(af)) => {
                        // RF audio: wire → Opus → reproductor (sin reproductor: no decodifica).
                        let frame = match arsp::audio_frame_from_wire(af) {
                            Ok(f) => f,
                            Err(e) => {
                                log.record(AuditEventKind::Error, session_id,
                                    serde_json::json!({"audio": e.to_string()}))?;
                                emit.emit(ev("audio_error", serde_json::json!({"detail": e.to_string()})));
                                continue;
                            }
                        };
                        if let (Some(codec), Some(pb)) = (codec.as_mut(), playback.as_ref()) {
                            match codec.decode(&frame.data) {
                                Ok(pcm) => {
                                    rms_acc += anyremote_audio::rms_i16(&pcm);
                                    rms_n += 1;
                                    pb.submit(pcm);
                                    audio_frames += 1;
                                    report.record_audio_recv(frame.data.len());
                                    if audio_frames % 100 == 1 {
                                        let (f, s) = pb.stats();
                                        let rms = if rms_n > 0 {
                                            ((rms_acc / rms_n as f64) * 10_000.0).round() / 10_000.0
                                        } else {
                                            0.0
                                        };
                                        emit.emit(ev("audio_stats",
                                            serde_json::json!({"frames": f, "samples": s, "rms": rms})));
                                        rms_acc = 0.0;
                                        rms_n = 0;
                                    }
                                }
                                Err(e) => {
                                    log.record(AuditEventKind::Error, session_id,
                                        serde_json::json!({"audio": e.to_string()}))?;
                                    emit.emit(ev("audio_error", serde_json::json!({"detail": e.to_string()})));
                                }
                            }
                        }
                    }
                    Some(proto::envelope::Payload::InputAck(a)) => {
                        // Acuse (RF-02a): el visor sabe si el anfitrión inyectó
                        // o por qué rechazó (contador de inyectadas + aviso).
                        emit.emit(ev("input_result", serde_json::json!({
                            "ok": a.ok, "perm": a.perm, "detail": a.detail,
                        })));
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
            _ = audio_tick.tick(), if playback.is_some() => {
                // Solo si YA llego audio antes y ahora no hay frames nuevos
                // (el primer tick es inmediato: no debe senalar silencio al arrancar).
                if audio_frames == stats_frames_mark && audio_frames > 0 {
                    let (f, s) = playback.as_ref().map(|p| p.stats()).unwrap_or((0, 0));
                    emit.emit(ev("audio_stats",
                        serde_json::json!({"frames": f, "samples": s, "rms": 0.0})));
                } else {
                    stats_frames_mark = audio_frames;
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
            line = bye_rx.recv(), if !bye_done => {
                let Some(line) = line else { bye_done = true; continue; };
                // RF-02a: el visor escribe eventos `{"input":...}` por stdin → al canal.
                if line.trim_start().starts_with('{') {
                    match parse_input_line(&line) {
                        Ok(Some(msg)) => {
                            if let Err(e) = ch.send(proto::envelope::Payload::InputEvent(msg)).await {
                                if conn_gone(&e) {
                                    log.record(AuditEventKind::Disconnected, session_id,
                                        serde_json::json!({"reason":"connection_gone","role":"peer"}))?;
                                    emit.emit(ev("closed", serde_json::json!({"reason": "connection_gone"})));
                                    break;
                                }
                                return Err(e.into());
                            }
                        }
                        Ok(None) => {} // JSON sin `input`: no es un evento de entrada
                        Err(detail) => {
                            emit.emit(ev("input_error", serde_json::json!({"detail": detail})));
                        }
                    }
                    continue;
                }
                let reason = if line.trim().is_empty() { "stdin".to_string() } else { line };
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
    log.record(
        AuditEventKind::Disconnected,
        session_id,
        report.disconnect_event(),
    )?;
    // La lectura final del portapapeles NO debe romper el cierre (modo flaky/bloqueo real).
    let clipboard_final = match engine.text() {
        Ok(t) => t,
        Err(e) => {
            log.record(
                AuditEventKind::Error,
                session_id,
                serde_json::json!({"clipboard": format!("{e:?}"), "dir":"final"}),
            )?;
            emit.emit(ev(
                "clipboard_error",
                serde_json::json!({"dir":"final","detail": format!("{e:?}")}),
            ));
            None
        }
    };
    Ok(RunOutcome {
        report: report.clone(),
        events: emit.events.clone(),
        clipboard_final,
    })
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
        .ok_or_else(|| {
            DaemonError::Profile(anyremote_profiles::ProfileError::NotFound(alias.into()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_gate_skips_only_sustained_silence() {
        // Sonido -> nunca omite y resetea el contador.
        let mut n = 0u32;
        assert!(!silence_gate(0.17, &mut n));
        assert_eq!(n, 0);
        // Silencio intermitente (10 frames) -> sigue enviando.
        for _ in 0..10 {
            assert!(!silence_gate(0.0, &mut n), "aun no debe omitir");
        }
        assert_eq!(n, 10);
        // Silencio sostenido (>100 frames = 2 s) -> omite.
        let mut skipped = false;
        for _ in 0..120 {
            if silence_gate(0.0, &mut n) {
                skipped = true;
                break;
            }
        }
        assert!(skipped, "tras 2 s de silencio debe omitir");
        // Vuelve el sonido -> reanuda envio.
        assert!(!silence_gate(0.05, &mut n));
        assert_eq!(n, 0);
    }
}
