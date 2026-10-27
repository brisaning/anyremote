//! `anyremote-audio` — audio de sesión: **captura WASAPI (cpal) → Opus 48 kHz
//! mono → reproducción**, con modo `Test` determinista para gates (patrón del
//! subsistema de vídeo). Clean-room D-01.
//!
//! Semilla validada (spike 2026-10-27): cpal enumeró/abrió micrófono y altavoz,
//! Opus codificó 1 s de tono a 52 kbps en ~20 ms y la reproducción sonó.
//!
//! **Requisito de build**: `opus` compila libopus con CMake — tener en `PATH`
//! `C:\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin`
//! (VS Build Tools) antes de `cargo build/test`.
//!
//! Diseño de hilos (WASAPI exige propietario del stream, y `cpal::Stream` no es
//! `Send`): cada dispositivo vive en su hilo y habla por canales; el codec Opus
//! (`Send`) vive en el daemon. El hilo solo hace E/S de dispositivo.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Fija de sesión: 48 kHz mono (formato soportado por Opus; WASAPI suele dar
/// 48 kHz en entrada y 44.1 kHz en salida → se remuestrea en ambos lados).
pub const AUDIO_RATE: u32 = 48_000;
/// 20 ms de PCM por frame (unidad Opus) @ 48 kHz.
pub const FRAME_SAMPLES: usize = 960;

#[derive(Debug, PartialEq, Eq)]
pub enum AudioError {
    Device(String),
    Codec(String),
    Config(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Modo de audio de sesión (espejo de `VideoMode`): `Off` = sin audio (defecto).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AudioMode {
    Off,
    /// Sin hardware: tono sintético (gates/e2e).
    Test,
    /// Dispositivo real por defecto (micrófono de entrada / altavoz de salida).
    On,
}

impl AudioMode {
    pub fn parse(s: &str) -> Result<Self, AudioError> {
        match s.to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "test" => Ok(Self::Test),
            "on" => Ok(Self::On),
            other => Err(AudioError::Config(format!(
                "--audio inválido: {other} (off|test|on)"
            ))),
        }
    }
}

/// Fuente de captura para `AudioMode::On` (solo el lado `listen`).
/// Por defecto **`System`**: el audio que suena en los altavoces (YouTube,
/// vídeos, llamadas) — el micrófono solo captura lo que ocurre en la sala.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AudioSource {
    /// Audio del sistema vía **WASAPI loopback** (endpoint de render como
    /// entrada: cpal activa `AUDCLNT_STREAMFLAGS_LOOPBACK` automáticamente).
    System,
    /// Micrófono (dispositivo de entrada por defecto).
    Microphone,
}

impl AudioSource {
    pub fn parse(s: &str) -> Result<Self, AudioError> {
        match s.to_ascii_lowercase().as_str() {
            "system" | "loopback" => Ok(Self::System),
            "mic" | "microphone" => Ok(Self::Microphone),
            other => Err(AudioError::Config(format!(
                "--audio-source inválido: {other} (system|mic)"
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Microphone => "mic",
        }
    }
}

impl Default for AudioSource {
    fn default() -> Self {
        Self::System
    }
}

/// RMS de un bloque de PCM normalizado (0..1) — diagnóstico de
/// "¿llega audio real o silencio?" en `audio_stats`.
pub fn rms_i16(pcm: &[i16]) -> f64 {
    if pcm.is_empty() {
        return 0.0;
    }
    let scale = 1.0 / i16::MAX as f64;
    (pcm.iter().map(|&s| (s as f64 * scale).powi(2)).sum::<f64>() / pcm.len() as f64).sqrt()
}

/// Frame de audio codificado listo para el wire (Opus, 48 kHz mono, 20 ms).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedAudio {
    pub seq: u64,
    pub data: Vec<u8>,
}

// ---------------------------------------------------------------- conversiones

/// Interleaved f32 → mono i16 (promedia canales y recorta a rango).
pub fn interleaved_to_mono_i16(samples: &[f32], channels: usize) -> Vec<i16> {
    if channels == 0 || samples.is_empty() {
        return Vec::new();
    }
    samples
        .chunks(channels)
        .map(|frame| {
            let v = frame.iter().sum::<f32>() / frame.len() as f32;
            (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
        })
        .collect()
}

/// Remuestreo lineal `from` → `to` (identidad si las tasas coinciden).
/// Calidad suficiente para ratios cercanos (44.1↔48); mejorable con un
/// resampler de mayor orden si el campo nota artificios.
pub fn resample_linear_i16(input: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to || input.is_empty() || from == 0 || to == 0 {
        return input.to_vec();
    }
    let out_len = (input.len() as u64 * to as u64 / from as u64) as usize;
    if out_len == 0 {
        return Vec::new();
    }
    let ratio = from as f64 / to as f64;
    let last = input.len() - 1;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let i0 = (pos as usize).min(last);
            let frac = (pos - i0 as f64) as f32;
            let a = input[i0] as f32;
            let b = input[(i0 + 1).min(last)] as f32;
            (a + (b - a) * frac) as i16
        })
        .collect()
}

/// Mono i16 → interleaved f32 con `channels` (upmix por duplicación).
pub fn mono_i16_to_interleaved_f32(pcm: &[i16], channels: usize) -> Vec<f32> {
    let ch = channels.max(1);
    let scale = 1.0 / i16::MAX as f32;
    let mut out = Vec::with_capacity(pcm.len() * ch);
    for &s in pcm {
        let v = s as f32 * scale;
        for _ in 0..ch {
            out.push(v);
        }
    }
    out
}

// ---------------------------------------------------------------- codec

fn codec_err(e: opus::Error) -> AudioError {
    AudioError::Codec(format!("{e:?}"))
}

/// Codec de sesión: Opus 48 kHz mono (`Send` → vive en el daemon).
pub struct OpusCodec {
    enc: opus::Encoder,
    dec: opus::Decoder,
    seq: u64,
}

impl OpusCodec {
    pub fn new() -> Result<Self, AudioError> {
        Ok(Self {
            enc: opus::Encoder::new(
                AUDIO_RATE,
                opus::Channels::Mono,
                opus::Application::Audio,
            )
            .map_err(codec_err)?,
            dec: opus::Decoder::new(AUDIO_RATE, opus::Channels::Mono).map_err(codec_err)?,
            seq: 0,
        })
    }

    /// Codifica UN frame de PCM (exactamente `FRAME_SAMPLES` muestras 48 kHz mono).
    pub fn encode(&mut self, pcm: &[i16]) -> Result<EncodedAudio, AudioError> {
        if pcm.len() != FRAME_SAMPLES {
            return Err(AudioError::Codec(format!(
                "pcm {} ≠ {FRAME_SAMPLES}",
                pcm.len()
            )));
        }
        let mut buf = vec![0u8; 4000];
        let n = self.enc.encode(pcm, &mut buf).map_err(codec_err)?;
        buf.truncate(n);
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        Ok(EncodedAudio { seq, data: buf })
    }

    /// Decodifica un frame Opus → PCM 48 kHz mono.
    pub fn decode(&mut self, data: &[u8]) -> Result<Vec<i16>, AudioError> {
        if data.is_empty() {
            return Err(AudioError::Codec("frame vacío".into()));
        }
        // Margen para frames de hasta 120 ms (5760 muestras).
        let mut out = vec![0i16; FRAME_SAMPLES * 6];
        let n = self.dec.decode(data, &mut out, false).map_err(codec_err)?;
        out.truncate(n);
        Ok(out)
    }
}

// ---------------------------------------------------------------- captura

/// Captura de audio → PCM 48 kHz mono (20 ms) hacia `out`.
/// `Test` genera un tono sin tocar hardware; `On` abre las fuentes pedidas
/// (mezclándolas si hay varias).
pub struct AudioCapture {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    warnings: Vec<String>,
}

impl AudioCapture {
    /// Inicia la captura con las fuentes dadas (`System` = loopback del
    /// sistema, `Microphone` = micrófono). Con **varias fuentes se mezclan**
    /// (suma por muestras a 48 kHz mono). `Test` genera un tono y las ignora.
    /// Si alguna fuente no está disponible se degrada (se avisa en
    /// [`AudioCapture::warnings`]); solo falla si NO abre ninguna.
    pub fn start(
        mode: AudioMode,
        sources: &[AudioSource],
        out: tokio::sync::mpsc::Sender<Vec<i16>>,
    ) -> Result<AudioCapture, AudioError> {
        if mode == AudioMode::Off {
            return Err(AudioError::Config("captura con AudioMode::Off".into()));
        }
        let stop = Arc::new(AtomicBool::new(false));
        if mode == AudioMode::Test {
            let stop2 = stop.clone();
            let thread = std::thread::spawn(move || test_capture_loop(out, stop2));
            return Ok(AudioCapture {
                stop,
                thread: Some(thread),
                warnings: Vec::new(),
            });
        }
        if sources.is_empty() {
            return Err(AudioError::Config("captura sin fuentes".into()));
        }
        // `On`: el hilo abre los dispositivos y notifica el resultado antes de
        // entrar en el bucle (el fallo llega en milisegundos).
        let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<String>, AudioError>>();
        let stop2 = stop.clone();
        let owned: Vec<AudioSource> = sources.to_vec();
        let thread = std::thread::spawn(move || {
            let mut opened = Vec::new();
            let mut warnings = Vec::new();
            for s in &owned {
                match open_capture(*s) {
                    Ok(o) => opened.push((*s, o)),
                    Err(e) => warnings.push(format!("{}: {e}", s.as_str())),
                }
            }
            if opened.is_empty() {
                let _ = tx.send(Err(AudioError::Device(warnings.join("; "))));
            } else {
                let _ = tx.send(Ok(warnings));
                capture_loop(opened, out, stop2);
            }
        });
        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(warnings)) => Ok(AudioCapture {
                stop,
                thread: Some(thread),
                warnings,
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(AudioError::Device("el dispositivo no respondió (3 s)".into()))
            }
        }
    }

    /// Fuentes que no se pudieron abrir (p. ej. sin micrófono): la captura
    /// continúa con las demás.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join(); // ≤ ~25 ms (bucles con sueño corto)
        }
    }
}

/// Dispositivo abierto (stream vivo + su buffer crudo + formato).
struct OpenedCapture {
    stream: cpal::Stream,
    raw: Arc<Mutex<Vec<f32>>>,
    channels: usize,
    rate: u32,
}

/// Abre la fuente de captura con SU configuración (WASAPI rechaza tasas ajenas
/// al formato de mezcla) y devuelve el stream vivo + buffer crudo.
///
/// **Loopback (fuente `System`)**: usar el endpoint de *render* como entrada
/// hace que cpal añada `AUDCLNT_STREAMFLAGS_LOOPBACK` → se captura TODO lo que
/// suena en los altavoces (YouTube, vídeos, llamadas). En un endpoint de render
/// el formato válido lo da `default_output_config` (`default_input_config`
/// rechaza los endpoints de render).
fn open_capture(source: AudioSource) -> Result<OpenedCapture, AudioError> {
    let host = cpal::default_host();
    let (dev, supported) = match source {
        AudioSource::System => {
            let d = host
                .default_output_device()
                .ok_or_else(|| AudioError::Device("sin dispositivo de salida (loopback)".into()))?;
            let c = d
                .default_output_config()
                .map_err(|e| AudioError::Device(e.to_string()))?;
            (d, c)
        }
        AudioSource::Microphone => {
            let d = host
                .default_input_device()
                .ok_or_else(|| AudioError::Device("sin dispositivo de entrada".into()))?;
            let c = d
                .default_input_config()
                .map_err(|e| AudioError::Device(e.to_string()))?;
            (d, c)
        }
    };
    let channels = supported.channels() as usize;
    let rate = supported.sample_rate().0;
    let cfg: cpal::StreamConfig = supported.into();
    let raw = Arc::new(Mutex::new(Vec::<f32>::new()));
    let cb = raw.clone();
    let stream = dev
        .build_input_stream(
            &cfg,
            move |data: &[f32], _| {
                if let Ok(mut g) = cb.lock() {
                    // Si nadie drena (>1 s) se descarta en vez de crecer sin límite.
                    if g.len() > AUDIO_RATE as usize {
                        g.clear();
                    }
                    g.extend_from_slice(data);
                }
            },
            |_| {}, // stream roto (p. ej. desconexión): la captura se detiene sola
            None,
        )
        .map_err(|e| AudioError::Device(e.to_string()))?;
    stream
        .play()
        .map_err(|e| AudioError::Device(e.to_string()))?;
    Ok(OpenedCapture {
        stream,
        raw,
        channels,
        rate,
    })
}

/// Mezcla por muestras: toma el mínimo disponible entre las fuentes CON datos
/// (suma con saturación) y lo descarta de cada una. Las fuentes vacías no
/// aportan → una fuente muda o atrasada no bloquea a la otra; la deriva de
/// relojes se acota en `capture_loop` (tope de buffer).
pub fn mix_available(sources: &mut [Vec<i16>]) -> Vec<i16> {
    let n = match sources
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.len())
        .min()
    {
        Some(n) => n.min(FRAME_SAMPLES * 4),
        None => return Vec::new(),
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let mut acc = 0i32;
        for s in sources.iter() {
            if i < s.len() {
                acc += s[i] as i32;
            }
        }
        out.push(acc.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
    }
    for s in sources.iter_mut() {
        if s.len() >= n {
            s.drain(..n);
        }
    }
    out
}

/// Drena los dispositivos → mono → resample a 48 kHz → **mezcla** → frames de
/// 960 muestras hacia `out`.
fn capture_loop(
    sources: Vec<(AudioSource, OpenedCapture)>,
    out: tokio::sync::mpsc::Sender<Vec<i16>>,
    stop: Arc<AtomicBool>,
) {
    struct Feed {
        frame_in: usize,
        acc: Vec<i16>,  // mono a la tasa del dispositivo
        pcm48: Vec<i16>, // ya a 48 kHz
    }
    let mut feeds: Vec<Feed> = sources
        .iter()
        .map(|(_, o)| Feed {
            frame_in: ((FRAME_SAMPLES as u64 * o.rate as u64) / AUDIO_RATE as u64).max(1) as usize,
            acc: Vec::new(),
            pcm48: Vec::new(),
        })
        .collect();
    let mut pending: Vec<i16> = Vec::new();
    let cap = FRAME_SAMPLES * 4; // tope por deriva de reloj entre fuentes
    while !stop.load(Ordering::Relaxed) {
        for (i, (_, opened)) in sources.iter().enumerate() {
            let chunk = match opened.raw.lock() {
                Ok(mut g) => std::mem::take(&mut *g),
                Err(_) => return,
            };
            let feed = &mut feeds[i];
            if !chunk.is_empty() {
                feed.acc
                    .extend_from_slice(&interleaved_to_mono_i16(&chunk, opened.channels));
            }
            while feed.acc.len() >= feed.frame_in {
                let head: Vec<i16> = feed.acc.drain(..feed.frame_in).collect();
                if opened.rate == AUDIO_RATE {
                    feed.pcm48.extend_from_slice(&head);
                } else {
                    feed.pcm48
                        .extend_from_slice(&resample_linear_i16(&head, opened.rate, AUDIO_RATE));
                }
            }
            if feed.pcm48.len() > cap {
                let excess = feed.pcm48.len() - cap;
                feed.pcm48.drain(..excess);
            }
        }
        let mut bufs: Vec<Vec<i16>> = feeds.iter_mut().map(|f| std::mem::take(&mut f.pcm48)).collect();
        let mixed = mix_available(&mut bufs);
        for (i, b) in bufs.into_iter().enumerate() {
            feeds[i].pcm48 = b; // devuelve lo no consumido (mezcla parcial)
        }
        if !mixed.is_empty() {
            pending.extend_from_slice(&mixed);
        }
        while pending.len() >= FRAME_SAMPLES {
            let frame: Vec<i16> = pending.drain(..FRAME_SAMPLES).collect();
            let _ = out.try_send(frame); // canal lleno → se descarta (backpressure)
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    for (_, s) in sources {
        drop(s.stream);
    }
}

/// Tono de prueba: 440 Hz a 48 kHz, frames exactos de 960 muestras.
fn test_capture_loop(out: tokio::sync::mpsc::Sender<Vec<i16>>, stop: Arc<AtomicBool>) {
    let mut phase = 0f64;
    let step = 2.0 * std::f64::consts::PI * 440.0 / AUDIO_RATE as f64;
    while !stop.load(Ordering::Relaxed) {
        let frame: Vec<i16> = (0..FRAME_SAMPLES)
            .map(|_| {
                let v = (phase.sin() * 8000.0) as i16;
                phase += step;
                v
            })
            .collect();
        let _ = out.try_send(frame);
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ---------------------------------------------------------------- reproducción

/// Reproductor de audio de sesión: recibe PCM 48 kHz mono y lo reproduce
/// (o solo lo cuenta en modo `Test`). Cuenta frames/muestras procesados.
pub struct AudioPlayback {
    stop: Arc<AtomicBool>,
    tx: std::sync::mpsc::SyncSender<Vec<i16>>,
    thread: Option<std::thread::JoinHandle<()>>,
    frames: Arc<AtomicU64>,
    samples: Arc<AtomicU64>,
}

impl AudioPlayback {
    pub fn start(mode: AudioMode) -> Result<AudioPlayback, AudioError> {
        if mode == AudioMode::Off {
            return Err(AudioError::Config("reproducción con AudioMode::Off".into()));
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<i16>>(100);
        let frames = Arc::new(AtomicU64::new(0));
        let samples = Arc::new(AtomicU64::new(0));
        if mode == AudioMode::Test {
            let (stop2, f2, s2) = (stop.clone(), frames.clone(), samples.clone());
            let thread = std::thread::spawn(move || playback_loop(rx, stop2, f2, s2, None));
            return Ok(AudioPlayback {
                stop,
                tx,
                thread: Some(thread),
                frames,
                samples,
            });
        }
        let (otx, orx) = std::sync::mpsc::channel::<Result<(), AudioError>>();
        let (stop2, f2, s2) = (stop.clone(), frames.clone(), samples.clone());
        let thread = std::thread::spawn(move || match open_playback() {
            Err(e) => {
                let _ = otx.send(Err(e));
            }
            Ok(device) => {
                let _ = otx.send(Ok(()));
                playback_loop(rx, stop2, f2, s2, Some(device));
            }
        });
        match orx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => Ok(AudioPlayback {
                stop,
                tx,
                thread: Some(thread),
                frames,
                samples,
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(AudioError::Device("el altavoz no respondió (3 s)".into()))
            }
        }
    }

    /// Encola PCM decodificado (no bloquea: `false` si la cola está llena).
    pub fn submit(&self, pcm: Vec<i16>) -> bool {
        self.tx.try_send(pcm).is_ok()
    }

    /// (frames procesados, muestras procesadas) — para `audio_stats`.
    pub fn stats(&self) -> (u64, u64) {
        (
            self.frames.load(Ordering::Relaxed),
            self.samples.load(Ordering::Relaxed),
        )
    }
}

impl Drop for AudioPlayback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

type PlaybackDevice = (cpal::Stream, Arc<Mutex<VecDeque<f32>>>, u32, usize);

/// Abre el altavoz por defecto con SU configuración (WASAPI) + buffer de salida.
fn open_playback() -> Result<PlaybackDevice, AudioError> {
    let host = cpal::default_host();
    let dev = host
        .default_output_device()
        .ok_or_else(|| AudioError::Device("sin dispositivo de salida".into()))?;
    let supported = dev
        .default_output_config()
        .map_err(|e| AudioError::Device(e.to_string()))?;
    let rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let cfg: cpal::StreamConfig = supported.into();
    let buf = Arc::new(Mutex::new(VecDeque::<f32>::new()));
    let cb = buf.clone();
    let stream = dev
        .build_output_stream(
            &cfg,
            move |out: &mut [f32], _| {
                let mut g = match cb.lock() {
                    Ok(g) => g,
                    Err(_) => {
                        out.fill(0.0);
                        return;
                    }
                };
                // Underrun → silencio; si el productor se desborda (>2 s) → vacía.
                if g.len() > rate as usize * 2 {
                    g.clear();
                }
                for s in out.iter_mut() {
                    *s = g.pop_front().unwrap_or(0.0);
                }
            },
            |_| {},
            None,
        )
        .map_err(|e| AudioError::Device(e.to_string()))?;
    stream
        .play()
        .map_err(|e| AudioError::Device(e.to_string()))?;
    Ok((stream, buf, rate, channels))
}

/// Consume PCM del canal: resample 48 kHz → tasa del dispositivo, upmix y
/// (en modo `Test`, sin dispositivo) solo contabiliza.
fn playback_loop(
    rx: std::sync::mpsc::Receiver<Vec<i16>>,
    stop: Arc<AtomicBool>,
    frames: Arc<AtomicU64>,
    samples: Arc<AtomicU64>,
    device: Option<PlaybackDevice>,
) {
    let mut device = device;
    while !stop.load(Ordering::Relaxed) {
        let pcm = match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(p) => p,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let n = pcm.len() as u64;
        frames.fetch_add(1, Ordering::Relaxed);
        samples.fetch_add(n, Ordering::Relaxed);
        if let Some((stream, buf, rate, channels)) = device.as_mut() {
            let converted = if *rate == AUDIO_RATE {
                mono_i16_to_interleaved_f32(&pcm, *channels)
            } else {
                let r = resample_linear_i16(&pcm, AUDIO_RATE, *rate);
                mono_i16_to_interleaved_f32(&r, *channels)
            };
            if let Ok(mut g) = buf.lock() {
                if g.len() > *rate as usize * 2 {
                    g.clear();
                }
                g.extend(converted);
            }
            let _ = stream; // el stream se consume al salir del bucle (Drop)
        }
    }
    if let Some((stream, ..)) = device.take() {
        drop(stream);
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_mode_parse_accepts_off_test_on() {
        assert_eq!(AudioMode::parse("off").unwrap(), AudioMode::Off);
        assert_eq!(AudioMode::parse("Test").unwrap(), AudioMode::Test);
        assert_eq!(AudioMode::parse("on").unwrap(), AudioMode::On);
        assert!(AudioMode::parse("surround").is_err());
    }

    #[test]
    fn audio_source_defaults_to_system_and_parses_aliases() {
        // El defecto es el audio del sistema: es lo que el usuario espera al
        // compartir (YouTube, vídeos); el micrófono es opt-in explícito.
        assert_eq!(AudioSource::default(), AudioSource::System);
        assert_eq!(AudioSource::parse("system").unwrap(), AudioSource::System);
        assert_eq!(AudioSource::parse("loopback").unwrap(), AudioSource::System);
        assert_eq!(AudioSource::parse("MIC").unwrap(), AudioSource::Microphone);
        assert!(AudioSource::parse("hdmi").is_err());
        assert_eq!(AudioSource::System.as_str(), "system");
    }

    #[test]
    fn rms_reports_silence_and_tone_levels() {
        assert_eq!(rms_i16(&[]), 0.0);
        assert_eq!(rms_i16(&[0i16; 960]), 0.0);
        // Tono a media escala → RMS ≈ amplitud/√2.
        let tone: Vec<i16> = (0..960)
            .map(|i| (16_000.0 * (i as f64 * 0.13).sin()) as i16)
            .collect();
        let r = rms_i16(&tone);
        assert!(
            (0.30..0.40).contains(&r),
            "RMS del tono fuera de rango: {r:.4}"
        );
    }

    #[test]
    fn downmix_averages_channels_and_upmix_duplicates() {
        // Estéreo (1.0, -1.0) → promedio 0.
        let mono = interleaved_to_mono_i16(&[1.0, -1.0, 0.5, 0.5], 2);
        assert_eq!(mono, vec![0, i16::MAX / 2]);
        // Mono → estéreo: cada muestra dos veces.
        let st = mono_i16_to_interleaved_f32(&[32767], 2);
        assert_eq!(st.len(), 2);
        assert!((st[0] - 1.0).abs() < 1e-6 && st[0] == st[1]);
        // Recorte de rango.
        assert_eq!(interleaved_to_mono_i16(&[2.0], 1), vec![i16::MAX]);
    }

    #[test]
    fn resample_is_identity_or_scales_length() {
        let src: Vec<i16> = (0..882).map(|i| (i % 100) as i16).collect();
        assert_eq!(resample_linear_i16(&src, 44_100, 44_100), src);
        let up = resample_linear_i16(&src, 44_100, 48_000);
        assert_eq!(up.len(), 960); // 882 × 48000/44100 = 960 exacto
        let down = resample_linear_i16(&vec![0i16; 960], 48_000, 44_100);
        assert_eq!(down.len(), 882);
    }

    #[test]
    fn opus_roundtrip_preserves_tone_energy() {
        let mut codec = OpusCodec::new().expect("opus");
        let mut decoded_total = 0usize;
        let mut input_rms = 0f64;
        let mut output_rms = 0f64;
        let mut phase = 0f64;
        let step = 2.0 * std::f64::consts::PI * 440.0 / AUDIO_RATE as f64;
        for _ in 0..50 {
            let frame: Vec<i16> = (0..FRAME_SAMPLES)
                .map(|_| {
                    let v = (phase.sin() * 10_000.0) as i16;
                    phase += step;
                    v
                })
                .collect();
            input_rms += frame.iter().map(|&s| (s as f64).powi(2)).sum::<f64>();
            let enc = codec.encode(&frame).expect("encode");
            assert!(!enc.data.is_empty());
            let out = codec.decode(&enc.data).expect("decode");
            output_rms += out.iter().map(|&s| (s as f64).powi(2)).sum::<f64>();
            decoded_total += out.len();
        }
        assert_eq!(decoded_total, 50 * FRAME_SAMPLES, "1 s decodificada");
        let ratio = (output_rms / input_rms).sqrt();
        assert!(
            (0.5..1.5).contains(&ratio),
            "energía conservada (ratio={ratio:.3})"
        );
    }

    #[test]
    fn test_capture_emits_full_pcm_frames() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<i16>>(8);
        let cap = AudioCapture::start(AudioMode::Test, &[AudioSource::default()], tx)
            .expect("capture");
        let frame = rx.blocking_recv().expect("frame");
        assert_eq!(frame.len(), FRAME_SAMPLES);
        assert!(frame.iter().any(|&s| s != 0), "el tono no debe ser silencio");
        drop(cap); // detiene el hilo
    }

    #[test]
    fn multi_source_capture_starts_in_test_mode() {
        // Varias fuentes (sistema + micrófono) → el tono de prueba fluye igual.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<i16>>(8);
        let cap = AudioCapture::start(
            AudioMode::Test,
            &[AudioSource::System, AudioSource::Microphone],
            tx,
        )
        .expect("capture multi-fuente");
        let frame = rx.blocking_recv().expect("frame");
        assert_eq!(frame.len(), FRAME_SAMPLES);
        assert!(cap.warnings().is_empty(), "el modo Test no abre dispositivos");
        drop(cap);
    }

    #[test]
    fn mix_available_sums_skips_empty_and_saturates() {
        // Solo una fuente con datos → pasa su señal sin bloquearse por la otra.
        let mut one = vec![vec![10, 20, 30], vec![]];
        assert_eq!(mix_available(&mut one), vec![10, 20, 30]);
        assert!(one.iter().all(|s| s.is_empty()));

        // Ambas con datos → suma por muestras; lo no consumido queda pendiente.
        let mut two = vec![vec![100, 200], vec![1, 2, 3]];
        assert_eq!(mix_available(&mut two), vec![101, 202]);
        assert_eq!(two[0], Vec::<i16>::new());
        assert_eq!(two[1], vec![3]);

        // Suma con saturación (no desborda i16).
        let mut sat = vec![vec![i16::MAX], vec![10]];
        assert_eq!(mix_available(&mut sat), vec![i16::MAX]);

        // Sin datos → nada.
        assert!(mix_available(&mut vec![Vec::<i16>::new(), Vec::new()]).is_empty());
    }

    #[test]
    fn test_playback_counts_frames_and_samples() {
        let pb = AudioPlayback::start(AudioMode::Test).expect("playback");
        for _ in 0..10 {
            assert!(pb.submit(vec![0i16; FRAME_SAMPLES]));
        }
        // El hilo contabiliza al procesar.
        let mut stats = (0, 0);
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(10));
            stats = pb.stats();
            if stats == (10, 10 * FRAME_SAMPLES as u64) {
                break;
            }
        }
        assert_eq!(stats, (10, 10 * FRAME_SAMPLES as u64));
        drop(pb);
    }
}
