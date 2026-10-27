//! Tarea de vídeo (Hito M2 + refinamiento): captura → escalado bilineal → preview
//! local → encode → cola al canal de sesión, con **control adaptativo de calidad**:
//! EMA del tiempo de encode vs presupuesto de frame → ajusta bitrate (VP9) o escala.
//!
//! La tarea se crea DENTRO de la tarea de sesión (el capturer concreto nunca cruza
//! hilos como trait object). Backpressure: si el canal está lleno, el frame se
//! descarta (la visualización no ahoga a la sesión).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyremote_capture::{
    best_capturer, Capturer, Frame, PixelFormat, PlatformCapturer, TestPatternCapturer,
};
use anyremote_codec::{encoder_for, CodecFormat, EncodedFrame, VideoEncoder};
use serde_json::json;
use tokio::sync::mpsc;

use crate::preview::PreviewServer;

/// Mensaje del task de vídeo hacia la sesión: frame a enviar o evento de control.
pub enum VideoMsg {
    Frame(EncodedFrame),
    Event(serde_json::Value),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VideoMode {
    Off,
    /// Mejor capturer disponible (DXGI con fallback a patrón de test).
    Auto,
    /// Patrón de test determinista (pruebas / demos sin escritorio).
    Test,
}

impl VideoMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "off" => Ok(Self::Off),
            "on" | "auto" => Ok(Self::Auto),
            "test" => Ok(Self::Test),
            other => Err(format!("--video inválido: off|on|test (recibido {other})")),
        }
    }
}

/// Escalado bilineal (BGRA8) — calidad de previsualización/streaming (M2+).
pub fn scale_frame(src: &Frame, w: u32, h: u32) -> Frame {
    if src.width == w && src.height == h {
        return src.clone();
    }
    let (sw, sh) = (src.width.max(1), src.height.max(1));
    let mut data = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let fy = (y as f32 + 0.5) * sh as f32 / h as f32 - 0.5;
        let y0 = fy.floor().max(0.0) as usize;
        let y1 = (y0 + 1).min(sh as usize - 1);
        let wy = (fy - y0 as f32).clamp(0.0, 1.0);
        for x in 0..w {
            let fx = (x as f32 + 0.5) * sw as f32 / w as f32 - 0.5;
            let x0 = fx.floor().max(0.0) as usize;
            let x1 = (x0 + 1).min(sw as usize - 1);
            let wx = (fx - x0 as f32).clamp(0.0, 1.0);
            let di = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                let p00 = src.data[(y0 * sw as usize + x0) * 4 + c] as f32;
                let p10 = src.data[(y0 * sw as usize + x1) * 4 + c] as f32;
                let p01 = src.data[(y1 * sw as usize + x0) * 4 + c] as f32;
                let p11 = src.data[(y1 * sw as usize + x1) * 4 + c] as f32;
                let top = p00 + (p10 - p00) * wx;
                let bot = p01 + (p11 - p01) * wx;
                let v = top + (bot - top) * wy;
                data[di + c] = v.round().clamp(0.0, 255.0) as u8;
            }
            data[di + 3] = 255;
        }
    }
    Frame {
        width: w,
        height: h,
        format: PixelFormat::Bgra8,
        data,
    }
}

/// Decisión del controlador adaptativo de calidad.
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct AdaptDecision {
    /// Nuevo tamaño de trabajo (redimensiona y recrea el encoder) si difiere.
    pub video_size: Option<(u32, u32)>,
    /// Nueva tasa objetivo (bps) si el códec la soporta.
    pub target_bps: Option<usize>,
    pub reason: &'static str,
}

/// Estado del control adaptativo (EMA del tiempo de encode vs presupuesto).
pub struct AdaptState {
    pub ema_ms: f64,
    pub bps: usize,
    pub size: (u32, u32),
    pub initial_size: (u32, u32),
    pub frames_since: u32,
    max_bps: usize,
    min_bps: usize,
}

impl AdaptState {
    /// `max_bps` (QW-3): tope de ancho de banda (bps). Sin tope, el techo es
    /// `bps*4` (el adaptativo puede rampar mucho en resoluciones altas).
    pub fn new(size: (u32, u32), max_bps: Option<usize>) -> Self {
        let base = (size.0 as usize * size.1 as usize * 3).max(150_000);
        let cap = max_bps.unwrap_or(base * 4).max(150_000);
        Self {
            ema_ms: 0.0,
            bps: base.min(cap),
            size,
            initial_size: size,
            frames_since: 0,
            max_bps: cap,
            min_bps: 200_000.min(cap),
        }
    }

    /// Observa un frame codificado; decide adaptar cada ~60 frames.
    pub fn observe(&mut self, encode_ms: f64, budget_ms: f64) -> Option<AdaptDecision> {
        self.ema_ms = if self.ema_ms <= 0.0 {
            encode_ms
        } else {
            0.8 * self.ema_ms + 0.2 * encode_ms
        };
        self.frames_since += 1;
        if self.frames_since < 60 {
            return None;
        }
        self.frames_since = 0;

        if self.ema_ms > 0.9 * budget_ms {
            // Presión: primero baja bitrate; si ya está en el suelo, baja escala.
            if self.bps > self.min_bps {
                self.bps = (self.bps / 2).max(self.min_bps);
                return Some(AdaptDecision {
                    video_size: None,
                    target_bps: Some(self.bps),
                    reason: "presion_bitrate",
                });
            }
            let (w0, h0) = self.size;
            let nw = ((w0 as f32 * 0.85) as u32).max(320);
            let nh = ((h0 as f32 * 0.85) as u32).max(180);
            if (nw, nh) != self.size {
                self.size = (nw, nh);
                self.bps = (nw as usize * nh as usize * 3).max(150_000);
                return Some(AdaptDecision {
                    video_size: Some(self.size),
                    target_bps: None,
                    reason: "presion_escala",
                });
            }
            return None;
        }
        if self.ema_ms < 0.4 * budget_ms {
            // Sobra capacidad: primero recupera escala, luego bitrate.
            if self.size != self.initial_size {
                self.size = self.initial_size;
                self.bps = (self.size.0 as usize * self.size.1 as usize * 3).max(150_000);
                return Some(AdaptDecision {
                    video_size: Some(self.size),
                    target_bps: None,
                    reason: "recupera_escala",
                });
            }
            if self.bps < self.max_bps {
                self.bps = (self.bps * 3 / 2).min(self.max_bps);
                return Some(AdaptDecision {
                    video_size: None,
                    target_bps: Some(self.bps),
                    reason: "recupera_bitrate",
                });
            }
        }
        None
    }
}

/// Resolución efectiva de trabajo: `requested==(0,0)` = **nativa** (la fija el
/// primer frame del capturer; en modo Test sin escritorio → 1280x720).
pub fn resolve_native(mode: VideoMode, requested: (u32, u32), first: (u32, u32)) -> (u32, u32) {
    if requested.0 > 0 && requested.1 > 0 {
        return requested;
    }
    if matches!(mode, VideoMode::Test) || first.0 == 0 || first.1 == 0 {
        return (1280, 720);
    }
    first
}

/// Lanza la tarea de vídeo hacia `out` (preview local + eventos). Deténela con `stop`.
pub fn spawn_video_task(
    mode: VideoMode,
    size: (u32, u32),
    fps: u64,
    codec: CodecFormat,
    max_bps: Option<usize>,
    out: mpsc::Sender<VideoMsg>,
    preview: PreviewServer,
    stop: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Resolución nativa (0,0): en Test se fija ya; en Auto espera al primer frame.
        let (mut w, mut h) = if size == (0, 0) && matches!(mode, VideoMode::Test) {
            (1280, 720)
        } else {
            size
        };
        let mut capturer: PlatformCapturer = match mode {
            VideoMode::Off => return,
            VideoMode::Test => PlatformCapturer::TestPattern(TestPatternCapturer::new(w.max(1), h.max(1))),
            VideoMode::Auto => best_capturer(w.max(64), h.max(64)),
        };
        let mut encoder = encoder_for(codec);
        // QW-4: el códec debe conocer el fps real (control de tasa).
        encoder.set_fps(fps.max(1) as u32);
        let mut adapt: Option<AdaptState> = None;
        let budget_ms = 1000.0 / fps.max(1) as f64;
        let mut seq: u32 = 0;
        let mut tick = tokio::time::interval(Duration::from_millis(1000 / fps.max(1)));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            tick.tick().await;
            let captured = match capturer.next_frame() {
                Ok(f) => f,
                Err(_) => continue, // captura momentáneamente no disponible
            };
            // Fija la resolución nativa con el primer frame.
            if w == 0 || h == 0 {
                w = captured.width;
                h = captured.height;
            }
            let frame = scale_frame(&captured, w, h);
            if adapt.is_none() {
                adapt = Some(AdaptState::new((w, h), max_bps));
                // QW-3: aplica el tope inicial (o el bitrate base) al encoder.
                if let Some(st) = adapt.as_ref() {
                    let _ = encoder.set_target_bitrate(st.bps);
                }
            }

            let t0 = std::time::Instant::now();
            let enc = match encoder.encode(&frame) {
                Ok(e) => e,
                Err(_) => continue,
            };
            let took_ms = t0.elapsed().as_secs_f64() * 1000.0;

            preview.push(frame.width, frame.height, seq, &frame.data);
            seq = seq.wrapping_add(1);
            if !enc.data.is_empty() {
                let _ = out.try_send(VideoMsg::Frame(enc)); // backpressure: descarte
            }

            // Control adaptativo de calidad (bitrate/escala) cada ~60 frames.
            if let Some(st) = adapt.as_mut() {
                if let Some(dec) = st.observe(took_ms, budget_ms) {
                    let mut applied = false;
                    if let Some(bps) = dec.target_bps {
                        if encoder.set_target_bitrate(bps) {
                            applied = true;
                            let _ = out.try_send(VideoMsg::Event(json!({
                                "event": "video_adapt", "reason": dec.reason, "bps": bps,
                                "size": format!("{}x{}", st.size.0, st.size.1),
                            })));
                        }
                    }
                    if !applied {
                        if let Some((nw, nh)) = dec.video_size {
                            w = nw;
                            h = nh;
                            if matches!(mode, VideoMode::Test) {
                                capturer =
                                    PlatformCapturer::TestPattern(TestPatternCapturer::new(w, h));
                            }
                            encoder = encoder_for(codec); // recreado con la nueva escala
                            encoder.set_fps(fps.max(1) as u32);
                            let _ = out.try_send(VideoMsg::Event(json!({
                                "event": "video_adapt", "reason": dec.reason,
                                "size": format!("{}x{}", w, h),
                            })));
                        }
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyremote_capture::{Frame, PixelFormat};

    fn gradient4x4() -> Frame {
        let mut data = vec![0u8; 4 * 4 * 4];
        for y in 0..4u32 {
            for x in 0..4u32 {
                let i = ((y * 4 + x) * 4) as usize;
                data[i] = (x as u8) * 85; // B: 0,85,170,255 (gradiente)
                data[i + 1] = (y as u8) * 85;
                data[i + 2] = 128;
                data[i + 3] = 255;
            }
        }
        Frame { width: 4, height: 4, format: PixelFormat::Bgra8, data }
    }

    #[test]
    fn resolve_native_uses_first_frame_or_fixed_size() {
        // Explícito WxH manda siempre.
        assert_eq!(resolve_native(VideoMode::Auto, (640, 360), (1920, 1080)), (640, 360));
        // Nativa + capturer real → dimensiones del primer frame.
        assert_eq!(resolve_native(VideoMode::Auto, (0, 0), (1920, 1080)), (1920, 1080));
        // Nativa en modo Test (sin escritorio) → 1280x720.
        assert_eq!(resolve_native(VideoMode::Test, (0, 0), (64, 64)), (1280, 720));
    }

    #[test]
    fn bilinear_downscale_produces_interpolated_values() {
        let src = gradient4x4();
        let out = scale_frame(&src, 2, 2);
        assert_eq!((out.width, out.height), (2, 2));
        assert_eq!(out.data.len(), 2 * 2 * 4);
        // Bilineal promedia el par (0,85) → ~42/43; el vecino más próximo daría 0 o 85.
        let b = out.data[0];
        assert!((40..=45).contains(&b), "esperaba interpolación ~43, llegó {b}");
        // Sin cambios → clon idéntico.
        let same = scale_frame(&src, 4, 4);
        assert_eq!(same, src);
    }

    #[test]
    fn max_bitrate_caps_initial_and_ceiling() {
        // Sin tope: techo = bps*4 (comportamiento previo).
        let free = AdaptState::new((1920, 1080), None);
        let base = (1920 * 1080 * 3) as usize;
        assert_eq!(free.bps, base);
        assert_eq!(free.max_bps, base * 4);
        // Con tope: el bitrate inicial y el techo respetan el tope (QW-3).
        let capped = AdaptState::new((1920, 1080), Some(1_000_000));
        assert_eq!(capped.bps, 1_000_000, "inicial acotado al tope");
        assert_eq!(capped.max_bps, 1_000_000);
        // El suelo nunca supera el tope.
        let tiny = AdaptState::new((320, 180), Some(100_000));
        assert!(tiny.min_bps <= tiny.max_bps);
        // Recuperación sin pasarse del tope.
        let mut st = AdaptState::new((640, 360), Some(500_000));
        st.bps = 400_000;
        let mut saw = None;
        for _ in 0..60 {
            if let Some(d) = st.observe(1.0, 66.0) {
                saw = Some(d);
                break;
            }
        }
        if let Some(d) = saw {
            if let Some(bps) = d.target_bps {
                assert!(bps <= 500_000, "no debe pasar del tope: {bps}");
            }
        }
    }

    #[test]
    fn adapt_low_bitrate_first_then_scale_under_pressure() {
        let mut s = AdaptState::new((960, 540), None);
        let budget = 66.0;
        let initial_bps = s.bps;
        // Presión sostenida: primero baja bitrate…
        let mut first = None;
        for _ in 0..60 {
            if let Some(d) = s.observe(budget * 2.0, budget) {
                first = Some(d);
                break;
            }
        }
        let d = first.expect("decisión en el primer umbral");
        assert_eq!(d.reason, "presion_bitrate");
        assert!(d.target_bps.unwrap() < initial_bps);
        // …hasta el suelo de bitrate → escala.
        let mut scale = None;
        for _ in 0..(60 * 6) {
            if let Some(d) = s.observe(budget * 2.0, budget) {
                if d.reason == "presion_escala" {
                    scale = Some(d);
                    break;
                }
            }
        }
        let d = scale.expect("llega a presion_escala tras bajar al suelo");
        assert!(d.video_size.is_some());
        // Con holgura: recupera escala y luego bitrate.
        let mut recovered = None;
        for _ in 0..(60 * 2) {
            if let Some(d) = s.observe(1.0, budget) {
                if d.reason == "recupera_escala" {
                    recovered = Some(d);
                    break;
                }
            }
        }
        assert!(recovered.expect("recupera_escala").video_size == Some((960, 540)));
    }
}
