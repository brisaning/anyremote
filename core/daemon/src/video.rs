//! Tarea de vídeo (Hito M2): captura → escala → preview local → encode → cola al canal.
//!
//! La tarea se crea DENTRO de la tarea de sesión (el capturer concreto nunca cruza
//! hilos como trait object). Backpressure: si el canal de sesión está lleno, el frame
//! se descarta (la visualización no ahoga a la sesión).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyremote_capture::{
    best_capturer, Capturer, Frame, PixelFormat, PlatformCapturer, TestPatternCapturer,
};
use anyremote_codec::{encoder_for, CodecFormat, EncodedFrame};
use tokio::sync::mpsc;

use crate::preview::PreviewServer;

pub struct VideoItem {
    pub enc: EncodedFrame,
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

/// Escalado por vecino más próximo (BGRA8) — suficiente para previsualización/stream M2.
pub fn scale_frame(src: &Frame, w: u32, h: u32) -> Frame {
    if src.width == w && src.height == h {
        return src.clone();
    }
    let mut data = vec![0u8; (w * h * 4) as usize];
    let (sw, sh) = (src.width.max(1), src.height.max(1));
    for y in 0..h {
        let sy = (y * sh / h) as usize;
        for x in 0..w {
            let sx = (x * sw / w) as usize;
            let si = (sy * sw as usize + sx) * 4;
            let di = ((y * w + x) * 4) as usize;
            if si + 4 <= src.data.len() && di + 4 <= data.len() {
                data[di..di + 4].copy_from_slice(&src.data[si..si + 4]);
            }
        }
    }
    Frame {
        width: w,
        height: h,
        format: PixelFormat::Bgra8,
        data,
    }
}

/// Lanza la tarea de vídeo hacia `out` (y preview local). Deténela con `stop`.
pub fn spawn_video_task(
    mode: VideoMode,
    size: (u32, u32),
    fps: u64,
    codec: CodecFormat,
    out: mpsc::Sender<VideoItem>,
    preview: PreviewServer,
    stop: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let (w, h) = size;
        let mut capturer: PlatformCapturer = match mode {
            VideoMode::Off => return,
            VideoMode::Test => PlatformCapturer::TestPattern(TestPatternCapturer::new(w, h)),
            VideoMode::Auto => best_capturer(w, h),
        };
        let mut encoder = encoder_for(codec);
        let mut seq: u32 = 0;
        let mut tick = tokio::time::interval(Duration::from_millis(1000 / fps.max(1)));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            tick.tick().await;
            let frame = match capturer.next_frame() {
                Ok(f) => f,
                Err(_) => continue, // captura momentáneamente no disponible
            };
            let frame = scale_frame(&frame, w, h);
            let enc = match encoder.encode(&frame) {
                Ok(e) => e,
                Err(_) => continue,
            };
            preview.push(frame.width, frame.height, seq, &frame.data);
            seq = seq.wrapping_add(1);
            if enc.data.is_empty() {
                continue; // frame omitido por el codificador (VP9)
            }
            let _ = out.try_send(VideoItem { enc }); // backpressure: descarte
        }
    })
}
