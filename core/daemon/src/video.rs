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

/// Nombre del backend realmente usado (evento `video_backend`, C-14).
fn backend_label(codec: CodecFormat) -> &'static str {
    match codec {
        CodecFormat::H264 => "mf_h264",
        CodecFormat::Vp9 => "vpx_vp9",
        CodecFormat::Deflate => "deflate",
        CodecFormat::Raw => "raw",
    }
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
///
/// C-31 (campo 17/11): versión separable con pesos enteros 16.16 y accesos
/// secuenciales. La anterior hacía, por cada píxel de salida, 4 lecturas
/// dispersas y 3 canales en `f32`; con escritorio 4K→720p el bucle de vídeo
/// (serie) bajaba a 1–5 fps en el anfitrión. Misma semántica: bilineal,
/// alfa = 255.
///
/// C-36 (campo 18/11): con el encoder en 8 ms, el escalado 4K→1080p pasó a ser
/// el cuello (**32–48 ms** por frame). Ahora las dos pasadas se reparten por
/// bandas entre hilos y el `scratch` se reutiliza entre frames
/// ([`FrameScaler`]).
pub struct FrameScaler {
    /// dw × sh × 4 — resultado de la pasada horizontal.
    scratch: Vec<u8>,
    xtab: Vec<(usize, usize, u32)>,
    ytab: Vec<(usize, usize, u32)>,
    /// (sw, sh, dw, dh) de las tablas vigentes.
    key: Option<(usize, usize, usize, usize)>,
}

impl Default for FrameScaler {
    fn default() -> Self {
        Self::new()
    }
}

/// Tabla de una eje: (índice0, índice1, peso 16.16) por píxel de salida.
fn axis_table(src_len: usize, dst_len: usize) -> Vec<(usize, usize, u32)> {
    let src = src_len.max(1) as f64;
    let dst = dst_len.max(1) as f64;
    let last = src_len.max(1) - 1;
    (0..dst_len)
        .map(|i| {
            let f = (i as f64 + 0.5) * src / dst - 0.5;
            let i0 = f.floor().max(0.0) as usize;
            let i1 = (i0 + 1).min(last);
            let w = ((f - i0 as f64).clamp(0.0, 1.0) * 65536.0).round() as u32;
            (i0, i1, w)
        })
        .collect()
}

impl FrameScaler {
    pub fn new() -> Self {
        Self {
            scratch: Vec::new(),
            xtab: Vec::new(),
            ytab: Vec::new(),
            key: None,
        }
    }

    fn tables(&mut self, sw: usize, sh: usize, dw: usize, dh: usize) {
        if self.key == Some((sw, sh, dw, dh)) {
            return;
        }
        self.xtab = axis_table(sw, dw);
        self.ytab = axis_table(sh, dh);
        self.key = Some((sw, sh, dw, dh));
    }

    /// Escala `src` a `w`×`h` (misma semántica que [`scale_frame`]).
    pub fn scale(&mut self, src: &Frame, w: u32, h: u32) -> Frame {
        if src.width == w && src.height == h {
            return src.clone();
        }
        let (sw, sh) = (src.width.max(1) as usize, src.height.max(1) as usize);
        let (dw, dh) = (w as usize, h as usize);
        self.tables(sw, sh, dw, dh);
        let need = dw * sh * 4;
        if self.scratch.len() < need {
            self.scratch.resize(need, 0);
        }
        let mut data = vec![0u8; dw * dh * 4];
        let Self {
            scratch,
            xtab,
            ytab,
            ..
        } = self;
        h_pass(&src.data, sw, dw, sh, xtab, &mut scratch[..need]);
        v_pass(&scratch[..need], dw, dh, ytab, &mut data);
        Frame {
            width: w,
            height: h,
            format: PixelFormat::Bgra8,
            data,
        }
    }
}

/// Escalado puntual (crea un [`FrameScaler`] de un solo uso).
pub fn scale_frame(src: &Frame, w: u32, h: u32) -> Frame {
    FrameScaler::new().scale(src, w, h)
}

/// Hilos útiles para repartir filas (0/1 = serie).
fn scale_threads(rows: usize) -> usize {
    if rows < 480 {
        return 1;
    }
    std::thread::available_parallelism()
        .map(|v| v.get())
        .unwrap_or(1)
        .min(8)
}

/// Pasada horizontal: `src` (sw×sh) → `out` (dw×sh), filas contiguas.
fn h_pass(
    src: &[u8],
    sw: usize,
    dw: usize,
    rows: usize,
    xtab: &[(usize, usize, u32)],
    out: &mut [u8],
) {
    let threads = scale_threads(rows);
    if threads <= 1 {
        h_rows(src, sw, dw, rows, xtab, out);
        return;
    }
    let band = rows.div_ceil(threads);
    std::thread::scope(|scope| {
        for (k, chunk) in out.chunks_mut(band * dw * 4).enumerate() {
            let r0 = k * band;
            let r1 = (r0 + band).min(rows);
            let part = &src[r0 * sw * 4..r1 * sw * 4];
            scope.spawn(move || h_rows(part, sw, dw, r1 - r0, xtab, chunk));
        }
    });
}

fn h_rows(
    src: &[u8],
    sw: usize,
    dw: usize,
    rows: usize,
    xtab: &[(usize, usize, u32)],
    out: &mut [u8],
) {
    for y in 0..rows {
        let row = &src[y * sw * 4..(y + 1) * sw * 4];
        let out_row = &mut out[y * dw * 4..(y + 1) * dw * 4];
        for (x, &(x0, x1, wx)) in xtab.iter().enumerate() {
            let (i0, i1, o) = (x0 * 4, x1 * 4, x * 4);
            for c in 0..3 {
                let a = row[i0 + c] as u32;
                let b = row[i1 + c] as u32;
                out_row[o + c] = ((a * (65536 - wx) + b * wx) >> 16).min(255) as u8;
            }
            out_row[o + 3] = 255;
        }
    }
}

/// Pasada vertical: `scratch` (dw×sh) → `out` (dw×dh).
fn v_pass(scratch: &[u8], dw: usize, dh: usize, ytab: &[(usize, usize, u32)], out: &mut [u8]) {
    let threads = scale_threads(dh);
    if threads <= 1 {
        v_rows(scratch, dw, dh, ytab, out);
        return;
    }
    let band = dh.div_ceil(threads);
    std::thread::scope(|scope| {
        for (k, chunk) in out.chunks_mut(band * dw * 4).enumerate() {
            let y0 = k * band;
            let y1 = (y0 + band).min(dh);
            let tab = &ytab[y0..y1];
            scope.spawn(move || v_rows(scratch, dw, y1 - y0, tab, chunk));
        }
    });
}

fn v_rows(scratch: &[u8], dw: usize, _rows: usize, ytab: &[(usize, usize, u32)], out: &mut [u8]) {
    for (y, &(y0, y1, wy)) in ytab.iter().enumerate() {
        let r0 = &scratch[y0 * dw * 4..(y0 + 1) * dw * 4];
        let r1 = &scratch[y1 * dw * 4..(y1 + 1) * dw * 4];
        let o = y * dw * 4;
        for x in 0..dw {
            let i = x * 4;
            for c in 0..3 {
                let a = r0[i + c] as u32;
                let b = r1[i + c] as u32;
                out[o + i + c] = ((a * (65536 - wy) + b * wy) >> 16).min(255) as u8;
            }
            out[o + i + 3] = 255;
        }
    }
}

// C-13: umbrales de la adaptación por RED (decisión documentada en
// `evidence/net-adapt-gate.md`). El WiFi en campo marcaba 57–782 ms y la LAN
// <10 ms: 200/100 separan congestión real de ruido. Confirmación de 3
// observaciones (los latidos llegan 1/s), recuperación a los 3 (C-52: antes 5)
// con 20 s de cooldown anti-oscilación. La escalera es la PROBADA (bitrate
// primero, escala después); el fps no se toca (`--video-fps` ya tiene su suelo).
/// C-27: exceso de RTT sobre el mínimo de la sesión — señal de congestión
/// RELATIVA. Con RTT absoluto, un enlace con latencia base alta (relay
/// ~335 ms) estaba siempre ≥ umbral → cascada permanente y suelo de
/// 200 kbps (campo 2026-11-14: 11 frames/78 s). Peer viejo sin baseline →
/// 0 → equivale al absoluto.
pub(crate) fn net_excess(rtt_ms: u32, rtt_min_ms: u32) -> u32 {
    rtt_ms.saturating_sub(rtt_min_ms)
}

const NET_RTT_HIGH_MS: u32 = 200;
const NET_RTT_LOW_MS: u32 = 100;
const NET_CONFIRM: u32 = 3;
/// C-52 (D2): **3** muestras seguidas en zona sana **o neutra** dan un escalón
/// de subida. Eran 5 sanas (C-A1) y con el jitter del relay (exceso de
/// 100–200 ms) la recuperación era inalcanzable: 2 min 35 s congelada en el
/// suelo (campo 20/11). El cooldown de 20 s se mantiene: es él, no el número de
/// muestras, el que evita la oscilación.
const NET_RECOVER: u32 = 3;
const NET_COOLDOWN: u32 = 20;

/// C-52 (D1): piso de legibilidad de **texto** en bits por píxel y frame
/// (C-32: 0,4–0,5). Es la magnitud que el usuario percibe; el bitrate y la
/// escala son solo los mandos que la sostienen. Decisión del usuario
/// (20/11): 0,45.
pub(crate) const TEXT_BPP_FLOOR: f64 = 0.45;

/// C-52: tamaño mínimo de la escalera. Por debajo, un escritorio no se lee
/// aunque sobre bitrate (sustituye al suelo absoluto de 200 kbps).
const MIN_LADDER_SIZE: (u32, u32) = (640, 360);

/// C-52: la escalera tampoco baja del 50 % del tamaño inicial (C-A2, campo
/// 15/11). El piso de bpp es el criterio real; este sigue siendo un tope.
const MIN_SCALE_FRACTION: f32 = 0.5;

/// C-52 (D1): tasa mínima (bps) para sostener [`TEXT_BPP_FLOOR`] a `w`×`h`@`fps`.
pub(crate) fn bpp_floor_bps(w: u32, h: u32, fps: u32) -> usize {
    (TEXT_BPP_FLOOR * w as f64 * h as f64 * fps.max(1) as f64).round() as usize
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
    /// C-52: fps nominal de la sesión — sin él no hay piso de bpp (el piso es
    /// `0,45 · w · h · fps`, no un número de kbps).
    fps: u32,
    max_bps: usize,
    /// C-52: suelo **absoluto** de la escalera = piso de bpp del tamaño mínimo
    /// permitido, acotado por el tope. Antes eran 200 kbps fijos: a 1001×563
    /// eso son 0,003 bpp, ~150× por debajo de la legibilidad.
    min_bps: usize,
    // C-13: estado de la adaptación por red (pista del controlador).
    net_high: u32,
    net_low: u32,
    net_cooldown: u32,
    /// true mientras la red esté congestionada: bloquea las recuperaciones
    /// de CPU (encode) para que las dos políticas no se peleen (oscilación).
    net_congested: bool,
}

impl AdaptState {
    /// `max_bps` (QW-3): tope de ancho de banda (bps). Sin tope, el techo es
    /// `w*h*3*4` como antes.
    ///
    /// C-52 (B2): la tasa inicial **ya no es `w*h*3`** (0,200 bpp a 1080p@15:
    /// texto ilegible desde el primer IDR) sino el piso de bpp del tamaño
    /// activo, acotado por el tope del usuario.
    pub fn new(size: (u32, u32), max_bps: Option<usize>) -> Self {
        // fps de referencia (sesiones de campo a 15); producción pasa el real
        // con `with_fps`.
        Self::with_fps(size, max_bps, 15)
    }

    /// C-52: variante con el fps real de la sesión (el piso de bpp lo exige).
    pub fn with_fps(size: (u32, u32), max_bps: Option<usize>, fps: u32) -> Self {
        let fps = fps.max(1);
        // Techo QW-3: se conserva la regla histórica para no inflar el
        // presupuesto de una sesión sana.
        let legacy = (size.0 as usize * size.1 as usize * 3).max(150_000);
        let cap = max_bps.unwrap_or(legacy * 4).max(150_000);
        // La tasa inicial nunca pasa el tope del usuario (el enlace manda);
        // si el tope no sostiene el piso, la escalera bajará la escala.
        let initial = bpp_floor_bps(size.0, size.1, fps).min(cap).max(1);
        let mut st = Self {
            ema_ms: 0.0,
            bps: initial,
            size,
            initial_size: size,
            frames_since: 0,
            fps,
            // El tope manda: si no sostiene el piso de bpp del tamaño inicial,
            // la escalera baja la ESCALA (nunca el bitrate por debajo del piso).
            max_bps: cap,
            min_bps: 0,
            net_high: 0,
            net_low: 0,
            net_cooldown: 0,
            net_congested: false,
        };
        let (fw, fh) = st.ladder_floor_size();
        st.min_bps = bpp_floor_bps(fw, fh, fps).min(st.max_bps).max(1);
        st
    }

    /// C-52: piso de bpp de un tamaño concreto, acotado por el tope (un enlace
    /// de 2 Mbps no puede sostener 13,9 Mbps aunque el tamaño los pida).
    fn floor_bps_of(&self, w: u32, h: u32) -> usize {
        bpp_floor_bps(w, h, self.fps).min(self.max_bps).max(1)
    }

    /// C-52: piso de bpp del tamaño **activo** — ningún escalón baja de aquí.
    fn floor_bps(&self) -> usize {
        self.floor_bps_of(self.size.0, self.size.1)
    }

    /// C-52 (D1): tasa que corresponde a un tamaño cuando la escalera cambia de
    /// escala. Antes era `w*h` (1 bit/px) con suelo de 200 kbps, y tras cinco
    /// escalones quedaba en 0,095 bpp (campo 20/11).
    fn size_bps(&self, w: u32, h: u32) -> usize {
        // El suelo absoluto de la escalera es el piso de bpp del tamaño mínimo
        // permitido: ningún tamaño puede quedar por debajo de él.
        self.floor_bps_of(w, h).max(self.min_bps)
    }

    /// C-52: tamaño mínimo al que puede llegar la escalera — el 50 % del
    /// inicial (C-A2) pero nunca por debajo de [`MIN_LADDER_SIZE`] ni por
    /// encima del propio inicial.
    fn ladder_floor_size(&self) -> (u32, u32) {
        let fw = ((self.initial_size.0 as f32 * MIN_SCALE_FRACTION) as u32).max(MIN_LADDER_SIZE.0);
        let fh = ((self.initial_size.1 as f32 * MIN_SCALE_FRACTION) as u32).max(MIN_LADDER_SIZE.1);
        (fw.min(self.initial_size.0), fh.min(self.initial_size.1))
    }

    /// C-52 (D1): un escalón de escala hacia abajo (85 %), con el piso de bpp
    /// del tamaño nuevo. `None` en el suelo de legibilidad: ahí solo cabe
    /// esperar la recuperación.
    fn scale_down(&mut self, reason: &'static str) -> Option<AdaptDecision> {
        let (w0, h0) = self.size;
        let nw = ((w0 as f32 * 0.85) as u32).max(320);
        let nh = ((h0 as f32 * 0.85) as u32).max(180);
        let (fw, fh) = self.ladder_floor_size();
        if nw < fw || nh < fh || (nw, nh) == self.size {
            return None; // suelo de legibilidad: solo bitrate/recuperación
        }
        self.size = (nw, nh);
        self.bps = self.size_bps(nw, nh);
        Some(AdaptDecision {
            video_size: Some(self.size),
            target_bps: None,
            reason,
        })
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
            // Presión (C-52): el bitrate no baja del piso de bpp del tamaño
            // activo; si ya está en él, se baja la ESCALA (menos píxeles a la
            // misma calidad por píxel) en vez de hundir la tasa.
            let floor = self.floor_bps();
            if self.bps > floor {
                self.bps = (self.bps / 2).max(floor);
                return Some(AdaptDecision {
                    video_size: None,
                    target_bps: Some(self.bps),
                    reason: "presion_bitrate",
                });
            }
            return self.scale_down("presion_escala");
        }
        // C-13: mientras la red está congestionada NO se recupera por CPU
        // (las dos políticas se pelearían → oscilación). La PRESIÓN de encode
        // sigue permitida (bajar siempre es válido).
        if self.ema_ms < 0.4 * budget_ms && !self.net_congested {
            // Sobra capacidad: primero recupera escala, luego bitrate.
            if self.size != self.initial_size {
                self.size = self.initial_size;
                // QW-3: el tope también rige al restaurar la escala (antes el
                // bitrate «saltaba» el tope justo aquí — fix en el mismo ciclo).
                // C-52: la escala restaurada exige el piso de bpp de SU tamaño,
                // o el texto vuelve a quedar borroso con el frame grande.
                self.bps = self.size_bps(self.size.0, self.size.1);
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

    /// C-13: observa el RTT (ms; **exceso** sobre el mínimo desde C-27)
    /// medido por el **controlador** (1/s).
    /// Política anti-oscilación (`specs/02` §7):
    /// - congestión: RTT ≥ alto en **3 muestras** → baja bitrate (nunca por
    ///   debajo del piso de bpp) o escala 85 % si ya está en el piso, y activa
    ///   **cooldown de 20 s**;
    /// - sanidad: RTT ≤ bajo **o zona neutra** en **3 muestras** (C-52) y
    ///   cooldown agotado → recupera escala o bitrate (+50 %, tope QW-3);
    /// - mientras haya congestión, `net_congested` bloquea las recuperaciones
    ///   de CPU (`observe`).
    ///
    /// Semántica de contadores: `net_high` solo se limpia con RTT sano o al
    /// decidir una recuperación (ventana nueva); `net_low` cuenta muestras
    /// sanas **y neutras** — es lo que hace alcanzable la recuperación con el
    /// jitter del relay (C-52/D2).
    pub fn observe_net(&mut self, rtt_ms: u32) -> Option<AdaptDecision> {
        if rtt_ms <= NET_RTT_LOW_MS {
            self.net_low = self.net_low.saturating_add(1);
            self.net_high = 0;
        } else if rtt_ms >= NET_RTT_HIGH_MS {
            self.net_high = self.net_high.saturating_add(1);
            self.net_low = 0;
        } else {
            // Zona neutral (C-A1 + C-52): antes SE BORRABA `net_low` → con el
            // jitter intermedio de un enlace real (WAN/relay) nunca alcanzaba la
            // recuperación y la escalera solo bajaba (campo 15/11). Desde C-52
            // **cuenta** para recuperar: es el estado normal de un relay sano.
            // `net_high` no se toca (la evidencia de congestión no se borra).
            self.net_low = self.net_low.saturating_add(1);
        }

        if self.net_high >= NET_CONFIRM {
            self.net_high = 0;
            self.net_cooldown = NET_COOLDOWN;
            self.net_congested = true;
            // C-52 (D1): el bitrate baja, pero nunca por debajo del piso de
            // bpp del tamaño activo; si ya está en el piso, baja la ESCALA.
            let floor = self.floor_bps();
            if self.bps > floor {
                self.bps = (self.bps / 2).max(floor);
                return Some(AdaptDecision {
                    video_size: None,
                    target_bps: Some(self.bps),
                    reason: "red_bitrate",
                });
            }
            // Ya en el piso de bpp → baja la escala (la escalera probada).
            return self.scale_down("red_escala");
        }

        if self.net_cooldown > 0 {
            self.net_cooldown -= 1;
        }
        if self.net_low >= NET_RECOVER && self.net_cooldown == 0 {
            self.net_low = 0;
            // C-52: la decisión abre ventana nueva; si no, dos muestras altas
            // previas + una posterior volverían a colapsar el escalón recién
            // recuperado (oscilación).
            self.net_high = 0;
            self.net_congested = false;
            if self.size != self.initial_size {
                self.size = self.initial_size;
                self.bps = self.size_bps(self.size.0, self.size.1);
                return Some(AdaptDecision {
                    video_size: Some(self.size),
                    target_bps: None,
                    reason: "red_recupera_escala",
                });
            }
            if self.bps < self.max_bps {
                self.bps = (self.bps * 3 / 2).min(self.max_bps);
                return Some(AdaptDecision {
                    video_size: None,
                    target_bps: Some(self.bps),
                    reason: "red_recupera_bitrate",
                });
            }
        }
        None
    }
}

/// C-52 (D3): ¿este escalón **sube** la tasa (recuperación)? Solo entonces hay
/// que refrescar: bajar la tasa no devuelve detalle y un IDR extra costaría un
/// pico de bitrate justo cuando la red ya va mal.
fn decision_needs_refresh(reason: &str) -> bool {
    matches!(reason, "recupera_bitrate" | "red_recupera_bitrate")
}

/// C-52 (D3): aplica una decisión de **tasa** al encoder y, si es una
/// recuperación, pide el refresco del contenido estático.
/// `set_target_bitrate` solo mueve el *techo* del control de tasa: lo que no
/// cambia se queda con la calidad del último IDR (campo 20/11: el texto siguió
/// en 0,030 bpp con el objetivo a 15 Mbps).
fn apply_target_bps(encoder: &mut dyn VideoEncoder, bps: usize, reason: &str) -> bool {
    if !encoder.set_target_bitrate(bps) {
        return false;
    }
    if decision_needs_refresh(reason) {
        let ok = encoder.request_refresh();
        // Una línea por DECISIÓN (no por frame): el registro no se inunda.
        eprintln!("[video] refresco solicitado: motivo=recuperacion (aceptado={ok})");
    }
    true
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

/// C-24: cadencia de reenvío del último frame en pantalla estática. Con
/// salidas del códec todavía vacías (calentamiento del MFT: n inputs antes
/// del primer slice) se reemite cada 200 ms en vez de 1 s para que el
/// encoder llegue a emitir aunque nadie toque el anfitrión.
fn reemit_gap(empty_run: u32) -> Duration {
    if empty_run > 0 {
        Duration::from_millis(200)
    } else {
        Duration::from_secs(1)
    }
}

/// C-23/C-24: frame inicial GDI con diagnóstico en stderr (una vez por
/// sesión — la app lo muestra como `[listener-err]` y en campo se ve si GDI
/// falló o dio la imagen).
fn cold_start_gdi(logged: &mut bool) -> Option<Frame> {
    match anyremote_capture::initial_snapshot() {
        Ok(f) => {
            if !*logged {
                eprintln!("[video] frame inicial GDI: {}x{}", f.width, f.height);
                *logged = true;
            }
            Some(f)
        }
        Err(e) => {
            if !*logged {
                eprintln!("[video] frame inicial GDI no disponible: {e:?}");
                *logged = true;
            }
            None
        }
    }
}

/// Lanza la tarea de vídeo hacia `out` (preview local + eventos). Deténela con `stop`.
///
/// **Hilo propio (QW-5, campo v0.7.2)**: la captura DXGI puede bloquear hasta
/// 500 ms en `AcquireNextFrame` cuando la pantalla estática no cambia — hacerlo
/// en un worker de tokio **estrangulaba el bucle de sesión** (vídeo y latidos
/// sin salir en equipos con pocos núcleos). Aquí la tarea vive en su propio
/// hilo: duerme con `std::thread::sleep` y usa `try_send` (canal sin bloqueo).
///
/// **Pantalla estática (C-4)**: DXGI solo entrega frames cuando hay cambios;
/// como mucho 1 vez por segundo se **reemite el último frame** para que el
/// visor no se quede ciego (QW-1 descarta los frames sin cambios).
#[allow(clippy::too_many_arguments)] // parámetros de sesión con dueño claro por capa.
pub fn spawn_video_task(
    mode: VideoMode,
    size: (u32, u32),
    fps: u64,
    codec: CodecFormat,
    max_bps: Option<usize>,
    out: mpsc::Sender<VideoMsg>,
    preview: PreviewServer,
    stop: Arc<AtomicBool>,
    // C-13: pista de red del controlador (RTT ms; 0 = sin datos).
    mut net_rx: tokio::sync::watch::Receiver<u32>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        // Resolución nativa (0,0): en Test se fija ya; en Auto espera al primer frame.
        let (mut w, mut h) = if size == (0, 0) && matches!(mode, VideoMode::Test) {
            (1280, 720)
        } else {
            size
        };
        let mut capturer: PlatformCapturer = match mode {
            VideoMode::Off => return,
            VideoMode::Test => {
                PlatformCapturer::TestPattern(TestPatternCapturer::new(w.max(1), h.max(1)))
            }
            VideoMode::Auto => best_capturer(w.max(64), h.max(64)),
        };
        let mut encoder = encoder_for(codec);
        // QW-4: el códec debe conocer el fps real (control de tasa).
        encoder.set_fps(fps.max(1) as u32);
        // C-14: backend efectivo → visible en el registro (y en el gate).
        let mut backend = backend_label(codec);
        let mut mf_failed = false;
        let _ = out.try_send(VideoMsg::Event(serde_json::json!({
            "event": "video_backend", "encoder": backend,
        })));
        let mut adapt: Option<AdaptState> = None;
        let mut scaler = FrameScaler::new();
        let budget_ms = 1000.0 / fps.max(1) as f64;
        let period = Duration::from_millis(1000 / fps.max(1).max(1));
        let mut seq: u32 = 0;
        // Último frame emitido + instante (para el reenvío en pantalla estática).
        let mut last_frame: Option<Frame> = None;
        let mut last_emit = std::time::Instant::now();
        // C-24: salidas de códec vacías seguidas (calentamiento) + log del GDI.
        let mut empty_run: u32 = 0;
        let mut gdi_logged = false;
        // C-28: diagnóstico de tasa cada5 s (campo: en qué etapa se pierden
        // los frames — captura real vs entraron al encode vs emitidos vs
        // entregados al canal de sesión).
        let mut fps_log_at = std::time::Instant::now();
        let mut c_cap: u64 = 0;
        let mut c_in: u64 = 0;
        let mut c_emit: u64 = 0;
        let mut c_queued: u64 = 0;
        // C-33: bytes emitidos en la ventana → tasa REAL por frame y bits por
        // píxel (el objetivo del encoder decía 15 Mbps y en campo llegaban
        // 265 kbps sin que el log lo delatara).
        let mut c_bytes: u64 = 0;
        // C-31: tiempos por etapa (ms) para localizar el cuello del bucle.
        let mut t_cap_ms: f64 = 0.0;
        let mut t_scale_ms: f64 = 0.0;
        let mut t_enc_ms: f64 = 0.0;

        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            // C-28: log de tasa (stderr → `[listener-err]` en el ui.log).
            if fps_log_at.elapsed() >= Duration::from_secs(5) {
                let secs = fps_log_at.elapsed().as_secs().max(1) as f64;
                let kb_frame = if c_emit > 0 {
                    c_bytes as f64 / c_emit as f64 / 1024.0
                } else {
                    0.0
                };
                // bpp = bits por píxel y frame con los frames EMITIDOS.
                let bpp = if c_emit > 0 && w > 0 && h > 0 {
                    c_bytes as f64 * 8.0 / (c_emit as f64 * w as f64 * h as f64)
                } else {
                    0.0
                };
                eprintln!(
                    "[video] fps ({} s): captura={} entraron={} emitidos={} entregados={} · captura_ms={:.1} escala_ms={:.1} encode_ms={:.1} · {:.0} kbps {:.1} KB/frame {:.3} bpp ({width}x{height})",
                    fps_log_at.elapsed().as_secs(),
                    c_cap, c_in, c_emit, c_queued,
                    if c_cap > 0 { t_cap_ms / c_cap as f64 } else { 0.0 },
                    if c_in > 0 { t_scale_ms / c_in as f64 } else { 0.0 },
                    if c_in > 0 { t_enc_ms / c_in as f64 } else { 0.0 },
                    c_bytes as f64 * 8.0 / secs / 1000.0,
                    kb_frame,
                    bpp,
                    width = w,
                    height = h,
                );
                c_cap = 0;
                c_in = 0;
                c_emit = 0;
                c_queued = 0;
                c_bytes = 0;
                t_cap_ms = 0.0;
                t_scale_ms = 0.0;
                t_enc_ms = 0.0;
                fps_log_at = std::time::Instant::now();
            }
            let t0 = std::time::Instant::now();
            let t_cap = std::time::Instant::now();
            let mut fresh_frame = false;
            let captured = match capturer.next_frame() {
                Ok(f) => {
                    c_cap += 1;
                    fresh_frame = true;
                    Some(f)
                }
                Err(anyremote_capture::CaptureError::Timeout) => {
                    // Pantalla estática: DXGI no entrega nada nuevo (QW-1 salta
                    // los frames de solo-cursor). Reemitir el último como mucho
                    // una vez por segundo para mantener viva la vista.
                    match last_frame.as_ref() {
                        // C-24: con el códec aún calentando (salidas vacías)
                        // se reemite cada 200 ms → el MFT recibe inputs y
                        // emite su primer slice aunque la pantalla no cambie.
                        Some(f) if last_emit.elapsed() >= reemit_gap(empty_run) => Some(f.clone()),
                        Some(_) => None,
                        // C-23: arranque en frío SIN imagen (pantalla estática
                        // o bloqueada) — DXGI no entrega nada hasta que algo
                        // cambie y el visor se quedaría en «esperando primer
                        // frame». GDI da la imagen inicial (log una vez).
                        None => cold_start_gdi(&mut gdi_logged),
                    }
                }
                // C-23: ídem en errores duros del backend (p. ej. acceso
                // perdido con el escritorio bloqueado) mientras no haya imagen.
                Err(_) if last_frame.is_none() => cold_start_gdi(&mut gdi_logged),
                Err(_) => None, // error transitorio del backend: se reintenta
            };
            if fresh_frame {
                t_cap_ms += t_cap.elapsed().as_secs_f64() * 1000.0;
            }
            let Some(captured) = captured else {
                let elapsed = t0.elapsed();
                if elapsed < period {
                    std::thread::sleep(period - elapsed);
                }
                continue;
            };
            c_in += 1;
            // Fija la resolución nativa con el primer frame.
            if w == 0 || h == 0 {
                w = captured.width;
                h = captured.height;
            }
            let t_scale = std::time::Instant::now();
            // C-36: scaler reutilizable (tablas + scratch) — el escalado 4K→1080p
            // era el cuello (32–48 ms) con el encoder ya en 8 ms.
            let frame = scaler.scale(&captured, w, h);
            t_scale_ms += t_scale.elapsed().as_secs_f64() * 1000.0;
            if adapt.is_none() {
                // C-52: el piso de bpp depende del fps real de la sesión.
                adapt = Some(AdaptState::with_fps((w, h), max_bps, fps as u32));
                // QW-3: aplica el tope inicial (o el bitrate base) al encoder.
                if let Some(st) = adapt.as_ref() {
                    let _ = encoder.set_target_bitrate(st.bps);
                }
            }

            let te = std::time::Instant::now();
            let enc = match encoder.encode(&frame) {
                Ok(e) => e,
                Err(e) => {
                    // C-14: si el backend H.264/MF no sirve en esta máquina
                    // (sin MFT sync utilizable), cae a VP9/libvpx de forma
                    // VISIBLE con evento `video_backend`.
                    if backend == "mf_h264" && !mf_failed {
                        mf_failed = true;
                        backend = "vpx_vp9";
                        encoder = encoder_for(CodecFormat::Vp9);
                        encoder.set_fps(fps.max(1) as u32);
                        if let Some(st) = adapt.as_ref() {
                            let _ = encoder.set_target_bitrate(st.bps);
                        }
                        let _ = out.try_send(VideoMsg::Event(serde_json::json!({
                            "event": "video_backend", "encoder": "vpx_vp9",
                            "detail": format!("fallback desde mf_h264: {e:?}"),
                        })));
                        match encoder.encode(&frame) {
                            Ok(e2) => e2,
                            Err(_) => {
                                let elapsed = t0.elapsed();
                                if elapsed < period {
                                    std::thread::sleep(period - elapsed);
                                }
                                continue;
                            }
                        }
                    } else {
                        let elapsed = t0.elapsed();
                        if elapsed < period {
                            std::thread::sleep(period - elapsed);
                        }
                        continue;
                    }
                }
            };
            let took_ms = te.elapsed().as_secs_f64() * 1000.0;
            t_enc_ms += took_ms;

            preview.push(frame.width, frame.height, seq, &frame.data);
            seq = seq.wrapping_add(1);
            last_emit = std::time::Instant::now();
            last_frame = Some(frame);
            if enc.data.is_empty() {
                // C-24: códec aún calentando → el reenvío en pantalla
                // estática se acelera (`reemit_gap`) hasta que emita.
                empty_run = empty_run.saturating_add(1);
            } else {
                empty_run = 0;
                c_emit += 1;
                c_bytes += enc.data.len() as u64;
                // backpressure: descarte (contado como emitido≠entregado)
                if out.try_send(VideoMsg::Frame(enc)).is_ok() {
                    c_queued += 1;
                }
            }

            // Control adaptativo de calidad: pista de RED (C-13, la observa
            // el controlador 1/s) o la observación de encode cada ~60 frames.
            let mut net_rtt = None;
            if net_rx.has_changed().unwrap_or(false) {
                net_rtt = Some(*net_rx.borrow_and_update());
            }
            if let Some(st) = adapt.as_mut() {
                let dec = match net_rtt {
                    Some(rtt) => st.observe_net(rtt),
                    None => None,
                }
                .or_else(|| st.observe(took_ms, budget_ms));
                if let Some(dec) = dec {
                    let mut applied = false;
                    if let Some(bps) = dec.target_bps {
                        // C-52 (D3): al recuperar la tasa se pide un IDR para
                        // que el contenido estático se recodifique con la
                        // calidad nueva.
                        if apply_target_bps(encoder.as_mut(), bps, dec.reason) {
                            applied = true;
                            let _ = out.try_send(VideoMsg::Event(json!({
                                "event": "video_adapt", "reason": dec.reason, "bps": bps,
                                "size": format!("{}x{}", st.size.0, st.size.1),
                                // C-40: RTT que disparó la decisión (sin él no
                                // se distingue congestión real de un pico).
                                "rtt": net_rtt,
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
                            // C-14: si MF ya falló, no se reintenta (evita
                            // oscilar entre backends); si no, vuelve a intentarlo.
                            encoder = if mf_failed {
                                encoder_for(CodecFormat::Vp9)
                            } else {
                                encoder_for(codec)
                            };
                            backend = if mf_failed {
                                "vpx_vp9"
                            } else {
                                backend_label(codec)
                            };
                            encoder.set_fps(fps.max(1) as u32); // QW-4
                            if let Some(st) = adapt.as_ref() {
                                // restaura la tasa vigente en el encoder nuevo
                                let _ = encoder.set_target_bitrate(st.bps);
                            }
                            // C-52 (D3): el cambio de escala NO necesita
                            // `request_refresh` — la reconstrucción del encoder
                            // ya produce SPS/PPS + IDR (MF configura el MFT de
                            // cero y VP9 arranca con `since_key == 0`). Se
                            // registra para que el campo lo pueda comprobar, pero
                            // no se duplica el IDR.
                            eprintln!(
                                "[video] refresco solicitado: motivo=escala (idr por reconstruccion)"
                            );
                            let _ = out.try_send(VideoMsg::Event(json!({
                                "event": "video_adapt", "reason": dec.reason,
                                "size": format!("{}x{}", w, h),
                            })));
                        }
                    }
                }
            }

            let elapsed = t0.elapsed();
            if elapsed < period {
                std::thread::sleep(period - elapsed);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyremote_capture::{Frame, PixelFormat};

    /// C-27: el exceso es relativo al mínimo (relay estable = sano).
    #[test]
    fn net_excess_is_relative_to_session_min() {
        assert_eq!(net_excess(335, 335), 0, "relay estable → sano");
        assert_eq!(net_excess(507, 335), 172, "pico real sobre el mínimo");
        assert_eq!(net_excess(50, 335), 0, "nunca negativo");
        assert_eq!(
            net_excess(300, 0),
            300,
            "sin baseline (peer viejo) = absoluto"
        );
    }

    /// C-24: la cadencia de reenvío se acelera mientras el códec calienta.
    #[test]
    fn reemit_gap_accelerates_while_codec_warms_up() {
        assert_eq!(reemit_gap(0), Duration::from_secs(1));
        assert!(reemit_gap(1) < Duration::from_millis(300));
        assert!(reemit_gap(u32::MAX) < Duration::from_millis(300));
    }

    /// C-A1 + C-52 (D2): la zona neutra (exceso 100–200 ms) **cuenta** para
    /// recuperar; con jitter intermedio (enlaces reales) la recuperación sigue
    /// alcanzable.
    #[test]
    fn net_low_counts_neutral_jitter_toward_recovery() {
        let mut s = AdaptState::new((1280, 720), Some(3_000_000));
        // C-52: con el tope (3 M) por debajo del piso de bpp de 720p, la tasa
        // ya nace en el piso → la respuesta a la congestión es ESCALA.
        assert_eq!(s.bps, 3_000_000, "tasa inicial = piso de bpp acotado");
        assert_eq!(s.observe_net(300), None, "1ª muestra");
        assert_eq!(s.observe_net(300), None, "2ª muestra");
        let step = s.observe_net(300).expect("3ª muestra → decisión");
        assert_eq!(step.reason, "red_escala", "el bitrate no baja del piso");
        assert!(step.video_size.is_some());
        assert!(s.net_congested);
        assert_eq!(s.net_cooldown, NET_COOLDOWN);
        // 5 sanas durante el cooldown → net_low=5 (bloqueado por cooldown).
        for _ in 0..5 {
            assert_eq!(s.observe_net(0), None, "cooldown activo");
        }
        assert_eq!(s.net_low, 5);
        // Jitter neutral (100–200): C-52 lo cuenta como evidencia de
        // recuperación (antes solo decaía, y con el relay la escalera no volvía).
        assert_eq!(s.observe_net(150), None);
        assert_eq!(s.net_low, 6, "la zona neutral suma, no resta");
        // Sano hasta agotar el cooldown → recupera la escala.
        let mut got = None;
        for _ in 0..20 {
            got = got.or_else(|| s.observe_net(0));
        }
        let d = got.expect("recupera pese al jitter neutro previo");
        assert_eq!(d.reason, "red_recupera_escala");
        assert_eq!(s.size, (1280, 720));
    }

    /// C-A2: la escalera no baja del50 % del inicial y el bitrate no se
    /// reinfla en cada escalón (campo15/11:6 escalas en2 min sin
    /// recuperación →850x478@200k, texto ilegible).
    #[test]
    fn scale_ladder_is_capped_and_does_not_reinflate() {
        let mut s = AdaptState::new((1920, 1080), Some(6_000_000));
        let mut escalas = 0;
        for _ in 0..150 {
            if let Some(d) = s.observe_net(500) {
                if d.reason == "red_escala" {
                    escalas += 1;
                    let (w, h) = s.size;
                    // C-52: el escalón fija el piso de bpp de ESE tamaño y no
                    // reinfla (antes: `w*h` con suelo de 200 kbps, que dejó el
                    // texto en 0,095 bpp tras cinco escalones).
                    assert_eq!(
                        s.bps,
                        bpp_floor_bps(w, h, 15).min(s.max_bps),
                        "escalón = piso de bpp de {w}x{h}"
                    );
                    assert!(s.bps <= s.max_bps, "respeta el tope: {}", s.bps);
                }
            }
        }
        assert!(escalas > 0 && escalas <= 6, "escalera acotada: {escalas}");
        assert!(
            s.size.0 >= 960 && s.size.1 >= 540,
            "nunca por debajo del50 %: {:?}",
            s.size
        );
    }

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
        Frame {
            width: 4,
            height: 4,
            format: PixelFormat::Bgra8,
            data,
        }
    }

    #[test]
    fn resolve_native_uses_first_frame_or_fixed_size() {
        // Explícito WxH manda siempre.
        assert_eq!(
            resolve_native(VideoMode::Auto, (640, 360), (1920, 1080)),
            (640, 360)
        );
        // Nativa + capturer real → dimensiones del primer frame.
        assert_eq!(
            resolve_native(VideoMode::Auto, (0, 0), (1920, 1080)),
            (1920, 1080)
        );
        // Nativa en modo Test (sin escritorio) → 1280x720.
        assert_eq!(
            resolve_native(VideoMode::Test, (0, 0), (64, 64)),
            (1280, 720)
        );
    }

    #[test]
    fn bilinear_scale_averages_2x2_block() {
        // 2x2 con valores conocidos → 1x1 = media exacta (bilineal en el centro).
        let mut data = vec![0u8; 2 * 2 * 4];
        data[0] = 0; // (0,0) B
        data[4] = 100; // (1,0) B
        data[8] = 200; // (0,1) B
        data[12] = 40; // (1,1) B
        for i in 0..4 {
            data[i * 4 + 3] = 255;
        }
        let src = Frame {
            width: 2,
            height: 2,
            format: PixelFormat::Bgra8,
            data,
        };
        let out = scale_frame(&src, 1, 1);
        // horiz: 50 y 120; vert: (50 + 120) / 2 = 85
        assert_eq!(out.data[0], 85);
        assert_eq!(out.data[3], 255, "alfa opaco");
        assert_eq!((out.width, out.height), (1, 1));
    }

    #[test]
    fn bilinear_downscale_produces_interpolated_values() {
        let src = gradient4x4();
        let out = scale_frame(&src, 2, 2);
        assert_eq!((out.width, out.height), (2, 2));
        assert_eq!(out.data.len(), 2 * 2 * 4);
        // Bilineal promedia el par (0,85) → ~42/43; el vecino más próximo daría 0 o 85.
        let b = out.data[0];
        assert!(
            (40..=45).contains(&b),
            "esperaba interpolación ~43, llegó {b}"
        );
        // Sin cambios → clon idéntico.
        let same = scale_frame(&src, 4, 4);
        assert_eq!(same, src);
    }

    #[test]
    fn net_adaptation_confirms_then_lowers_and_recovers_after_cooldown() {
        let mut s = AdaptState::new((1280, 720), Some(3_000_000));
        let start = s.bps;
        assert_eq!(start, 3_000_000, "C-52: tope por debajo del piso de bpp");
        // Confirmación: 1 y 2 muestras altas → nada aún.
        assert_eq!(s.observe_net(300), None, "1ª muestra");
        assert_eq!(s.observe_net(300), None, "2ª muestra");
        // 3ª → en el piso de bpp del tamaño activo, baja ESCALA (C-52: el
        // bitrate solo baja hasta el piso; por debajo miente).
        let d = s.observe_net(300).expect("3ª muestra → decisión");
        assert_eq!(d.reason, "red_escala");
        assert_eq!(s.bps, start, "el bitrate no se hunde");
        assert!(d.video_size.is_some());
        assert!(s.bps >= s.floor_bps(), "nunca por debajo del piso de bpp");
        assert!(s.net_congested, "marca congestión");
        // RTT en zona neutral (100–200) NO borra la evidencia de alta…
        assert_eq!(s.observe_net(150), None);
        // …pero una muestra sana sí limpia net_high (sin re-accionar).
        assert_eq!(s.observe_net(0), None);
        assert_eq!(s.net_high, 0);
        // Cooldown: ni las primeras muestras sanas recuperan.
        for i in 0..15 {
            assert_eq!(s.observe_net(0), None, "cooldown activo (obs {i})");
        }
        // Sano sostenido + cooldown agotado → recupera la escala (primero la
        // escala, luego el bitrate: la escalera probada).
        let mut got = None;
        for _ in 0..15 {
            got = got.or_else(|| s.observe_net(0));
        }
        let d = got.expect("recupera tras cooldown");
        assert_eq!(d.reason, "red_recupera_escala");
        assert_eq!(s.size, (1280, 720));
        assert_eq!(s.bps, start, "la escala restaurada exige su piso de bpp");
        assert!(!s.net_congested, "recuperación desbloquea la CPU");
    }

    #[test]
    fn net_congestion_at_floor_scales_down_and_blocks_cpu_recovery() {
        // Tope por encima del piso de 960x540@15 (3,5 Mbps) para que la
        // escalera tenga margen: el suelo de bpp no es un número de kbps.
        let mut s = AdaptState::new((960, 540), Some(6_000_000));
        assert_eq!(s.bps, 3_499_200, "tasa inicial = piso de bpp de 960x540@15");
        let floor_size = s.size;
        assert_eq!(s.observe_net(500), None);
        assert_eq!(s.observe_net(500), None);
        let d = s.observe_net(500).expect("en el piso de bpp → escala");
        assert_eq!(d.reason, "red_escala");
        assert!(d.video_size.is_some());
        assert!(s.size != s.initial_size);
        assert_eq!(s.size, (816, 459), "un escalón del 85 %");
        assert!(s.bps >= s.floor_bps(), "el tamaño nuevo fija su piso");
        // Con CPU holgada, la recuperación de encode está BLOQUEADA (C-13):
        for _ in 0..60 {
            assert_eq!(
                s.observe(1.0, 100.0),
                None,
                "CPU no recupera con red saturada"
            );
        }
        // La recuperación de RED desbloquea todo (cooldown + sanidad).
        let mut got = None;
        for _ in 0..30 {
            got = got.or_else(|| s.observe_net(0));
        }
        let d = got.expect("la red recupera");
        assert_eq!(d.reason, "red_recupera_escala");
        assert_eq!(s.size, floor_size);
        assert!(!s.net_congested);
        // Ahora sí: con el bitrate POR DEBAJO del tope, la CPU vuelve a
        // poder recuperar (prueba el desbloqueo + el tope QW-3).
        let mut cpu = None;
        for _ in 0..60 {
            cpu = cpu.or_else(|| s.observe(1.0, 100.0));
        }
        let d = cpu.expect("la CPU recupera tras desbloquear");
        assert_eq!(d.reason, "recupera_bitrate");
        assert!(d.target_bps.unwrap() <= 6_000_000, "respeta el tope QW-3");
    }

    #[test]
    fn scale_recovery_respects_the_bitrate_cap() {
        // Fix en este ciclo: restaurar la escala no debe «saltarse» el tope.
        let mut s = AdaptState::new((960, 540), Some(400_000));
        s.size = (640, 360);
        s.bps = 300_000;
        let mut got = None;
        for _ in 0..60 {
            got = got.or_else(|| s.observe(1.0, 100.0));
        }
        let d = got.expect("recupera escala");
        assert_eq!(d.reason, "recupera_escala");
        assert!(s.bps <= 400_000, "el tope rige también aquí: {}", s.bps);
    }

    #[test]
    fn max_bitrate_caps_initial_and_ceiling() {
        // C-52 (B2): sin tope, la tasa inicial es el piso de bpp del tamaño
        // (0,45 · w · h · fps), no `w*h*3` (0,200 bpp a 1080p@15: ilegible).
        let free = AdaptState::new((1920, 1080), None);
        let base = (1920 * 1080 * 3) as usize;
        assert_eq!(free.bps, 13_996_800, "piso de bpp de 1920x1080@15");
        assert_eq!(free.max_bps, base * 4, "techo QW-3 sin cambios");
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
        // C-52: la tasa nace EN el piso de bpp, así que «bitrate primero» solo
        // aplica cuando una recuperación ya la subió (estado real tras un
        // escalón de subida). Se simula ese punto de partida.
        s.bps = 5_000_000;
        let above = s.bps;
        // Presión sostenida: primero baja bitrate, pero nunca del piso…
        let mut first = None;
        for _ in 0..60 {
            if let Some(d) = s.observe(budget * 2.0, budget) {
                first = Some(d);
                break;
            }
        }
        let d = first.expect("decisión en el primer umbral");
        assert_eq!(d.reason, "presion_bitrate");
        assert!(d.target_bps.unwrap() < above);
        assert_eq!(d.target_bps, Some(3_499_200), "clava el piso, no lo pasa");
        // …y al llegar al piso → escala.
        let mut scale = None;
        for _ in 0..(60 * 6) {
            if let Some(d) = s.observe(budget * 2.0, budget) {
                if d.reason == "presion_escala" {
                    scale = Some(d);
                    break;
                }
            }
        }
        let d = scale.expect("llega a presion_escala tras bajar al piso");
        assert!(d.video_size.is_some());
        assert!(s.bps >= s.floor_bps(), "el tamaño nuevo fija su piso");
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

/// C-52 · arnés de simulación determinista (SOLO tests).
///
/// Reproduce la escalera observada en el campo del 20/11/2026 con los MISMOS
/// números del log del anfitrión y comprueba **qué hace la política corregida
/// (Fase B)**: piso de bpp por tamaño, refresco al recuperar y recuperación
/// alcanzable con el jitter del relay.
///
/// Es la política real (`AdaptState`, `apply_target_bps`) más un modelo del
/// códec entre frames. Evidencia: `evidence/video-quality-regression.md`.
#[cfg(test)]
mod c52_ladder {
    use super::*;
    use anyremote_codec::CodecError;

    /// Serie de exceso de RTT observada en la sesión 2 del 20/11 (anfitrión
    /// Brian, 09:52:38–09:54:32): un valor por decisión de campo
    /// (`ui.log` líneas 5118…5168). Todos ≥ 200 ms → zona de congestión; los
    /// valores 250 son los que el log **no** imprime (las decisiones de
    /// escala no llevan `rtt`).
    const FIELD_RTT_EXCESS: &[u32] = &[
        529, 275, 234, 230, 794, 227, 296, 250, 267, 265, 237, 250, 286, 294, 250, 205, 232, 250,
        234, 329,
    ];

    /// Decisiones que la escalera **sin piso de bpp** produjo en campo (mismo
    /// orden, tasas y tamaños): 20 de red + 1 de presión de CPU — `ui.log`
    /// 5118…5168. Se conservan como evidencia: la Fase B las invalida a
    /// propósito (todas por debajo de 0,45 bpp a partir del escalón 3).
    const FIELD_DECISIONS: &[(&str, Option<usize>, (u32, u32))] = &[
        ("red_bitrate", Some(7_500_000), (1920, 1080)),
        ("red_bitrate", Some(3_750_000), (1920, 1080)),
        ("red_bitrate", Some(1_875_000), (1920, 1080)),
        ("red_bitrate", Some(937_500), (1920, 1080)),
        ("red_bitrate", Some(468_750), (1920, 1080)),
        ("red_bitrate", Some(234_375), (1920, 1080)),
        ("red_bitrate", Some(200_000), (1920, 1080)),
        ("red_escala", None, (1632, 918)),
        ("red_bitrate", Some(749_088), (1632, 918)),
        ("red_bitrate", Some(374_544), (1632, 918)),
        ("red_bitrate", Some(200_000), (1632, 918)),
        ("red_escala", None, (1387, 780)),
        ("presion_bitrate", Some(540_930), (1387, 780)),
        ("red_bitrate", Some(270_465), (1387, 780)),
        ("red_bitrate", Some(200_000), (1387, 780)),
        ("red_escala", None, (1178, 663)),
        ("red_bitrate", Some(400_000), (1178, 663)),
        ("red_bitrate", Some(200_000), (1178, 663)),
        ("red_escala", None, (1001, 563)),
        ("red_bitrate", Some(400_000), (1001, 563)),
        ("red_bitrate", Some(200_000), (1001, 563)),
    ];

    /// fps de las dos sesiones de campo (`listen --video-fps 15`).
    const FIELD_FPS: u32 = 15;

    /// bpp (bits por píxel y frame) que permite una tasa objetivo a un tamaño.
    fn bpp_of(rate_bps: usize, w: u32, h: u32, fps: u32) -> f64 {
        rate_bps as f64 / fps as f64 / (w as f64 * h as f64)
    }

    /// Una decisión de la escalera: (razón, tasa pedida, tasa vigente, tamaño).
    type Decision = (&'static str, Option<usize>, usize, (u32, u32));

    /// C-52: invariante del gate D1 — la tasa vigente nunca queda por debajo del
    /// piso de bpp del tamaño activo (acotado al tope del enlace).
    fn assert_bpp_floor(st: &AdaptState, cap: usize) {
        let floor = bpp_floor_bps(st.size.0, st.size.1, FIELD_FPS);
        assert!(
            st.bps >= floor.min(cap),
            "bps {} < piso {} ({}x{})",
            st.bps,
            floor.min(cap),
            st.size.0,
            st.size.1
        );
        if floor <= cap {
            let bpp = bpp_of(st.bps, st.size.0, st.size.1, FIELD_FPS);
            assert!(
                bpp >= TEXT_BPP_FLOOR - 0.005,
                "bpp {bpp:.3} < {TEXT_BPP_FLOOR} a {}x{} ({} bps)",
                st.size.0,
                st.size.1,
                st.bps
            );
        }
    }

    /// Reproduce la sesión 2 del 20/11 sobre la política **corregida**: 3
    /// muestras de RTT congestionadas por decisión de campo (NET_CONFIRM=3) y,
    /// en el punto del log, la observación de CPU que produjo `presion_bitrate`
    /// (encode_ms=110,2 ms con presupuesto 1000/15=66,7 ms).
    fn replay_session2_collapse() -> (AdaptState, Vec<Decision>) {
        let mut st = AdaptState::with_fps((1920, 1080), Some(15_000_000), FIELD_FPS);
        // Antes del colapso la escalera había subido a 15 Mbps (09:52:34).
        st.bps = 15_000_000;
        let mut got: Vec<Decision> = Vec::new();
        for (i, &rtt) in FIELD_RTT_EXCESS.iter().enumerate() {
            // 3 muestras altas → 1 decisión de red (`NET_CONFIRM`).
            for _ in 0..NET_CONFIRM {
                if let Some(d) = st.observe_net(rtt) {
                    got.push((d.reason, d.target_bps, st.bps, st.size));
                }
            }
            // En el punto del log el anfitrión registró `presion_bitrate`
            // (encode 110,2 ms > 0,9 × 66,7 ms) — 60 observaciones de CPU.
            if i == 11 {
                let budget = 1000.0 / FIELD_FPS as f64;
                for _ in 0..60 {
                    if let Some(d) = st.observe(110.2, budget) {
                        got.push((d.reason, d.target_bps, st.bps, st.size));
                    }
                }
            }
        }
        (st, got)
    }

    /// C-52 (B2): la escalera de la sesión 1 ya **no arranca** en `w*h*3`
    /// (0,200 bpp, texto ilegible desde el primer IDR): arranca en el piso de
    /// bpp (13 996 800 = 0,450 bpp a 1080p@15) y sube al tope con las mismas
    /// muestras sanas. Los escalones de campo (`ui.log:4436,4439,4480` desde
    /// 6 220 800) quedan como evidencia del defecto.
    #[test]
    fn c52_field_ladder_starts_at_the_text_floor() {
        let mut st = AdaptState::with_fps((1920, 1080), Some(15_000_000), FIELD_FPS);
        assert_eq!(st.bps, 13_996_800, "tasa inicial = piso de bpp (0,450)");
        assert!(
            bpp_of(6_220_800, 1920, 1080, FIELD_FPS) < TEXT_BPP_FLOOR,
            "la tasa inicial del campo (w*h*3) era ilegible: 0,200 bpp"
        );
        let mut ladder: Vec<(&str, usize)> = Vec::new();
        // 3 muestras sanas (exceso ≤100 ms) → 1 recuperación (NET_RECOVER=3).
        for _ in 0..15 {
            if let Some(d) = st.observe_net(0) {
                ladder.push((d.reason, d.target_bps.expect("decisión de tasa")));
            }
        }
        assert_eq!(
            ladder,
            vec![("red_recupera_bitrate", 15_000_000)],
            "del piso de bpp al tope CLI (`--max-bitrate 15000`)"
        );
    }

    /// **Gate D1** `ladder_preserves_bpp`: ningún escalón de la escalera —ni el
    /// de arranque ni los de escala, bajo congestión de red o presión de CPU—
    /// fija una tasa por debajo de 0,45 bpp del tamaño activo. Si el enlace no
    /// sostiene ese piso, la política **baja la escala**, no el bitrate.
    #[test]
    fn ladder_preserves_bpp() {
        let cap = 15_000_000usize;
        let mut st = AdaptState::with_fps((1920, 1080), Some(cap), FIELD_FPS);
        // Arranque: piso de bpp (antes, `w*h*3` = 0,200 bpp).
        assert_bpp_floor(&st, cap);
        let mut rungs = 0;
        // Congestión sostenida: la escalera entera (bitrate y escala).
        for _ in 0..(FIELD_RTT_EXCESS.len() * NET_CONFIRM as usize + 60) {
            if st.observe_net(500).is_some() {
                rungs += 1;
                assert_bpp_floor(&st, cap);
            }
        }
        // Presión de CPU: la otra rama de la política.
        for _ in 0..(60 * 6) {
            if st.observe(200.0, 1000.0 / FIELD_FPS as f64).is_some() {
                rungs += 1;
                assert_bpp_floor(&st, cap);
            }
        }
        assert!(rungs >= 4, "la escalera se movió: {rungs} escalones");
        // Suelo: mismo 50 % del inicial que en campo (1001x563), pero con el
        // piso de bpp en vez de 200 kbps → 3,8 Mbps (0,450 bpp) en lugar de
        // 0,003 bpp.
        assert_eq!(st.size, (1001, 563), "suelo de escala (50 % del inicial)");
        assert!(
            st.bps >= 3_804_050,
            "el suelo es de bpp, no de kbps: {} bps",
            st.bps
        );
        // Enlace que NO sostiene el piso de 1080p (2 Mbps): se baja la escala
        // en vez de hundir el bitrate; el bpp queda en el máximo que el tope
        // permite y la tasa nunca pasa el tope.
        let mut slow = AdaptState::with_fps((1920, 1080), Some(2_000_000), FIELD_FPS);
        assert_eq!(slow.bps, 2_000_000, "el tope manda sobre el piso");
        for _ in 0..90 {
            slow.observe_net(500);
            assert!(slow.bps <= 2_000_000, "respeta el tope: {}", slow.bps);
            assert_bpp_floor(&slow, 2_000_000);
        }
        assert!(
            slow.size.0 < 1920,
            "bajó escala en vez de hundir el bitrate: {:?}",
            slow.size
        );
    }

    /// C-52 (D1): la MISMA serie de RTT del colapso de la sesión 2 ya no lleva
    /// la escalera al suelo ilegible. Las 21 decisiones del campo
    /// (`FIELD_DECISIONS`) se conservan como evidencia: 17 tasas quedaron por
    /// debajo del piso de texto.
    #[test]
    fn c52_session2_replay_keeps_the_bpp_floor() {
        // Evidencia del defecto: escalones de TASA del campo bajo el piso.
        let below = FIELD_DECISIONS
            .iter()
            .filter_map(|(_, bps, (w, h))| bps.map(|bps| bpp_of(bps, *w, *h, FIELD_FPS)))
            .filter(|bpp| *bpp < TEXT_BPP_FLOOR)
            .count();
        assert!(
            below >= 15,
            "el campo vivió {below} escalones de tasa por debajo del piso"
        );
        // La política corregida, con la misma serie de RTT: nunca bajo el piso.
        let (st, got) = replay_session2_collapse();
        assert!(!got.is_empty(), "la escalera reacciona al RTT de campo");
        for (i, (reason, target, bps, size)) in got.iter().enumerate() {
            let floor = bpp_floor_bps(size.0, size.1, FIELD_FPS).min(15_000_000);
            assert!(
                *bps >= floor,
                "decisión #{} ({reason}) a {bps} bps < piso {floor}",
                i + 1
            );
            let bpp = bpp_of(*bps, size.0, size.1, FIELD_FPS);
            assert!(
                bpp >= TEXT_BPP_FLOOR - 0.005,
                "decisión #{} ({reason}) a {bpp:.3} bpp",
                i + 1
            );
            if let Some(t) = target {
                assert_eq!(Some(*t), Some(*bps), "la tasa pedida es la vigente");
            }
        }
        assert_eq!(st.size, (1001, 563), "mismo suelo de escala que en campo");
        assert!(
            bpp_of(st.bps, st.size.0, st.size.1, FIELD_FPS) >= TEXT_BPP_FLOOR - 0.005,
            "el suelo conserva el texto legible: {} bps",
            st.bps
        );
    }

    /// **Gate D2** `recovery_reachable_under_jitter`: la zona neutra (100–200 ms
    /// de exceso) cuenta para recuperar y **3** muestras seguidas dan un escalón
    /// de subida. En campo, con la exigencia de 5 sanas, la escalera quedó
    /// congelada **2 min 35 s** (155 muestras) en el suelo.
    #[test]
    fn recovery_reachable_under_jitter() {
        let (mut st, _) = replay_session2_collapse();
        let floor_state = (st.size, st.bps);
        // 155 muestras de jitter neutro (lo que duró el bloqueo en campo): debe
        // recuperar en cuanto el cooldown lo permita, no quedarse congelada.
        let mut recovered = None;
        let mut at = 0usize;
        let mut decisions = 0usize;
        for i in 0..155 {
            if let Some(d) = st.observe_net(150) {
                decisions += 1;
                if recovered.is_none() {
                    recovered = Some(d);
                    at = i + 1;
                }
            }
        }
        let d = recovered.expect("con jitter neutro la recuperación es alcanzable");
        assert_eq!(d.reason, "red_recupera_escala");
        assert!(
            at <= (NET_COOLDOWN + NET_RECOVER) as usize,
            "tarda {at} muestras (cooldown {NET_COOLDOWN} + {NET_RECOVER})"
        );
        assert!(
            st.size.0 > floor_state.0 .0 && st.size.1 > floor_state.0 .1,
            "sube al menos un escalón: {:?} → {:?}",
            floor_state.0,
            st.size
        );
        // Con 155 muestras neutras se recupera del todo: escala inicial y tope.
        assert_eq!(st.size, st.initial_size, "vuelve a la resolución inicial");
        assert_eq!(st.bps, 15_000_000, "y al tope de enlace");
        assert!(decisions >= 2, "escalones de subida: {decisions}");

        // Tres muestras neutras consecutivas bastan para el siguiente escalón.
        let mut st = AdaptState::with_fps((1920, 1080), Some(15_000_000), FIELD_FPS);
        st.size = (1001, 563);
        st.bps = bpp_floor_bps(1001, 563, FIELD_FPS);
        let mut step = None;
        for _ in 0..NET_RECOVER {
            step = st.observe_net(150).or(step);
        }
        let d = step.expect("3 muestras neutras → un escalón de subida");
        assert_eq!(d.reason, "red_recupera_escala");
        assert_eq!(st.size, (1920, 1080), "escala inicial restaurada");
        assert_eq!(st.bps, 13_996_800, "con el piso de bpp de 1080p@15");
    }

    /// Encoder espía: registra lo que producción le pide. Sirve para comprobar
    /// que el **camino real** (`apply_target_bps`) pide el refresco al
    /// recuperar y no en los demás escalones.
    #[derive(Default)]
    struct SpyEncoder {
        rate: usize,
        refreshes: u32,
    }

    impl VideoEncoder for SpyEncoder {
        fn encode(&mut self, frame: &Frame) -> Result<EncodedFrame, CodecError> {
            Ok(EncodedFrame {
                format: CodecFormat::H264,
                seq: 0,
                width: frame.width,
                height: frame.height,
                data: Vec::new(),
            })
        }

        fn format(&self) -> CodecFormat {
            CodecFormat::H264
        }

        fn set_target_bitrate(&mut self, bps: usize) -> bool {
            self.rate = bps;
            true
        }

        fn request_refresh(&mut self) -> bool {
            self.refreshes += 1;
            true
        }
    }

    /// Modelo del códec entre frames (H.264/NVENC por MFT) con lo observado en
    /// `core/codec/src/mf_backend.rs` y `vpx_backend.rs`:
    /// - CBR con mean = max = tasa objetivo;
    /// - GOP de `fps*2` entradas → un IDR cada `fps*2` (30 a 15 fps);
    /// - un P-frame sólo codifica lo que cambia; un IDR recodifica todo;
    /// - `set_target_bitrate` sólo cambia el **techo**: el refresco llega por
    ///   `request_refresh` (`CODECAPI_AVEncVideoForceKeyFrame` / flag de
    ///   libvpx) o por la **reconstrucción** del encoder al cambiar de escala.
    struct InterCodec {
        w: u32,
        h: u32,
        fps: u32,
        gop: u32,
        rate_bps: usize,
        inputs: u32,
        /// bpp con el que se codificó la última vez cada región.
        static_bpp: f64,
        moving_bpp: f64,
        /// IDRs por GOP (automáticos) y por refresco/reconstrucción.
        idrs: u32,
        refreshes: u32,
    }

    impl InterCodec {
        fn new(w: u32, h: u32, fps: u32, rate_bps: usize) -> Self {
            let bpp = bpp_of(rate_bps, w, h, fps);
            Self {
                w,
                h,
                fps,
                gop: (fps * 2).max(8),
                rate_bps,
                inputs: 0,
                static_bpp: bpp,
                moving_bpp: bpp,
                idrs: 1,
                refreshes: 0,
            }
        }

        fn rate_bpp(&self) -> f64 {
            bpp_of(self.rate_bps, self.w, self.h, self.fps)
        }

        /// `set_target_bitrate` del MFT: cambia el techo y nada más.
        fn set_target_bitrate(&mut self, bps: usize) {
            self.rate_bps = bps;
        }

        /// C-52: `request_refresh` aceptado → el próximo frame es IDR y
        /// recodifica TODO a la tasa vigente.
        fn request_refresh(&mut self) -> bool {
            self.refreshes += 1;
            self.inputs = 0;
            self.static_bpp = self.rate_bpp();
            self.moving_bpp = self.rate_bpp();
            true
        }

        /// C-52: cambiar de escala reconstruye el encoder (`encoder_for`): el
        /// MFT se configura de cero (SPS/PPS + IDR) o VP9 arranca con
        /// `since_key == 0` → **IDR sin pedirlo**.
        fn reconstruct(&mut self, w: u32, h: u32, rate_bps: usize) {
            self.w = w;
            self.h = h;
            self.gop = (FIELD_FPS * 2).max(8);
            self.rate_bps = rate_bps;
            self.inputs = 0;
            self.static_bpp = self.rate_bpp();
            self.moving_bpp = self.rate_bpp();
            self.refreshes += 1;
        }

        /// Codifica un frame: lo que cambia se recodifica a la tasa vigente;
        /// lo estático conserva su calidad hasta el próximo IDR.
        fn encode(&mut self, changed: bool) {
            self.inputs += 1;
            if self.inputs >= self.gop {
                self.inputs = 0;
                self.idrs += 1;
                self.static_bpp = self.rate_bpp();
            }
            if changed {
                self.moving_bpp = self.rate_bpp();
            }
        }
    }

    /// **Gate D3** `static_text_refreshes_after_recovery`: tras recuperar
    /// bitrate el encoder recibe una petición de refresco y el contenido
    /// estático se recodifica a la tasa nueva. Sin ella conservaba la calidad
    /// del último IDR (campo 20/11: bloques a 0,026 bpp con 15 Mbps de
    /// objetivo, «movimiento fluido con la imagen degradada»).
    #[test]
    fn static_text_refreshes_after_recovery() {
        // 1) El camino de PRODUCCIÓN pide el refresco solo al recuperar.
        let mut st = AdaptState::with_fps((1920, 1080), Some(15_000_000), FIELD_FPS);
        let mut rec = None;
        for _ in 0..NET_RECOVER {
            rec = st.observe_net(0).or(rec);
        }
        let d = rec.expect("3 muestras sanas → recuperación");
        assert_eq!(d.reason, "red_recupera_bitrate");
        let mut spy = SpyEncoder::default();
        assert!(apply_target_bps(&mut spy, d.target_bps.unwrap(), d.reason));
        assert_eq!(spy.rate, 15_000_000, "la tasa se aplicó");
        assert_eq!(
            spy.refreshes, 1,
            "el encoder recibe la petición de refresco"
        );
        // Un escalón de bajada NO refresca (un IDR en el peor momento cuesta
        // un pico de bitrate)…
        let mut spy = SpyEncoder::default();
        assert!(apply_target_bps(&mut spy, 7_500_000, "red_bitrate"));
        assert_eq!(spy.refreshes, 0);
        // …y el cambio de escala tampoco lo PIDE: la reconstrucción del
        // encoder ya produce IDR (no se duplica).
        assert!(!decision_needs_refresh("red_escala"));
        assert!(!decision_needs_refresh("recupera_escala"));

        // 2) Modelo: sin la petición, el texto estático conserva la calidad del
        // IDR viejo (lo observado en S1); con ella se recodifica a la nueva.
        let mut codec = InterCodec::new(1920, 1080, FIELD_FPS, 6_220_800);
        codec.set_target_bitrate(13_996_800);
        codec.encode(true);
        assert!(
            (codec.static_bpp - 0.200).abs() < 0.002,
            "sin refresco el texto conserva la calidad vieja: {} bpp",
            codec.static_bpp
        );
        assert!(
            (codec.moving_bpp - 0.450).abs() < 0.002,
            "lo que cambia sí sigue la tasa nueva: {} bpp",
            codec.moving_bpp
        );
        assert!(codec.request_refresh());
        codec.encode(false);
        assert!(
            (codec.static_bpp - 0.450).abs() < 0.002,
            "con refresco el texto se recodifica a la tasa nueva: {} bpp",
            codec.static_bpp
        );
        assert_eq!(codec.refreshes, 1);

        // 3) El cambio de escala refresca por reconstrucción: mismo resultado
        // que un IDR forzado y sin una petición extra.
        let mut codec = InterCodec::new(1920, 1080, FIELD_FPS, 13_996_800);
        codec.encode(false);
        assert!((codec.static_bpp - 0.450).abs() < 0.002);
        // `video.rs` aplica `video_size` → encoder nuevo con el piso de bpp del
        // tamaño nuevo.
        codec.reconstruct(1001, 563, bpp_floor_bps(1001, 563, FIELD_FPS));
        assert!(
            (codec.static_bpp - 0.450).abs() < 0.002,
            "la reconstrucción recodifica el texto: {} bpp",
            codec.static_bpp
        );
        assert_eq!(codec.refreshes, 1, "no se pide un IDR extra");
    }
}
