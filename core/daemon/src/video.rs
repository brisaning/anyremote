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

// -- C-52 Fase B2: el presupuesto lo fija el ENLACE, no el piso de bpp ------
//
// El gate de campo del 23/11 (`evidence/c52-field-gate-failed.md`) demostró
// que pedir el piso de bpp a ciegas satura el socket: 13,8 Mbps pedidos sobre
// ~1 Mbps entregados → la entrada del controlador quedó encolada detrás del
// vídeo y la app dejó de responder. La regla desde B2 es una sola:
// **el objetivo nunca supera lo que el enlace entrega**.

/// C-52 (B2/D1'): fracción de la tasa ENTREGADA que puede pedir el encoder. El
/// 20 % restante es la cabeza que necesita el control, que comparte socket en
/// TCP.
pub(crate) const DELIVERY_MARGIN: f64 = 0.8;

/// C-52 (B2/D1'): tasa de arranque (bps) antes de la primera medición de
/// entrega. Antes se arrancaba en el piso de bpp (13,9 Mbps a 1080p@15): en un
/// enlace de 1 Mbps eso son ~14 s de vídeo encolado antes del primer ajuste.
/// Se arranca conservador y se sube con evidencia (el piso solo es un techo de
/// calidad, no una exigencia inicial).
const INITIAL_TARGET_BPS: usize = 2_000_000;

/// C-52 (B2/D1'): suelo operativo del encoder. Si el enlace no llega ni al
/// piso de legibilidad, el objetivo baja **por debajo** de él (fail-open del
/// texto) pero nunca a cero: el control tiene que seguir vivo.
const MIN_TARGET_BPS: usize = 150_000;

/// C-52 (B2/D2'): histéresis entre cambios de **escala** (subir o bajar). El
/// campo del 23/11 cambió de escala 6 veces en 30 s (bpp 0,477 → 0,112 →
/// 0,610 → 0,496 → 0,121 → 0,734); cada cambio reconstruye el encoder y cuesta
/// un pico de bitrate justo cuando el enlace ya va mal.
pub(crate) const SCALE_HYSTERESIS_MS: f64 = 10_000.0;

/// C-52 (B2/P1): presupuesto de UNA escritura de vídeo por el canal de control
/// (TCP). Por encima, el buffer del socket está lleno y todo lo que se encole
/// detrás (input, audio, latidos) espera.
pub(crate) const VIDEO_WRITE_BUDGET_MS: f64 = 100.0;

/// C-52 (B2/P1): tiempo que se deja de alimentar vídeo tras una escritura
/// lenta, para que el socket drene y el control pase. El visor prefiere frames
/// frescos: descartar es mejor que encolar.
const VIDEO_DROP_HOLD_MS: f64 = 1_000.0;

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

/// C-52 (B2): dimensión PAR. El pipeline 4:2:0 (`codec/src/conv.rs`) rechaza
/// dimensiones impares y la escalera del campo las producía (1387x780,
/// 1001x563): al 85 % de un ancho par sale impar.
fn even(v: u32) -> u32 {
    v & !1
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
    /// C-52 (B2/D1'): última tasa ENTREGADA medida por el transporte (bps).
    /// `None` = aún sin medición (solo rige el tope del usuario).
    delivered_bps: Option<usize>,
    /// C-52 (B2/D2'): reloj de la política. Avanza 1 s por muestra de RTT (el
    /// controlador publica el RTT a 1/s), que es la cadencia real de la
    /// escalera; no se usa `Instant` para que la política siga siendo pura y
    /// determinista en los gates.
    now_ms: f64,
    /// C-52 (B2/D2'): instante en que se permite el próximo cambio de escala.
    scale_hold_until_ms: f64,
}

impl AdaptState {
    /// `max_bps` (QW-3): tope de ancho de banda (bps). Sin tope, el techo es
    /// `w*h*3*4` como antes.
    ///
    /// C-52 (B2/D1'): la tasa inicial **ya no es el piso de bpp a ciegas**. El
    /// gate de campo del 23/11 midió 13,8 Mbps pedidos sobre un enlace de
    /// ~1 Mbps: el encoder inundaba el socket compartido con el control y la
    /// app dejaba de responder. Se arranca en [`INITIAL_TARGET_BPS`] (o en el
    /// piso si el tope es menor) y se **sube con evidencia de entrega**.
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
        let initial = bpp_floor_bps(size.0, size.1, fps)
            .min(cap)
            .min(INITIAL_TARGET_BPS)
            .max(MIN_TARGET_BPS.min(cap));
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
            delivered_bps: None,
            now_ms: 0.0,
            scale_hold_until_ms: 0.0,
        };
        let (fw, fh) = st.ladder_floor_size();
        st.min_bps = bpp_floor_bps(fw, fh, fps).min(st.max_bps).max(1);
        st
    }

    /// C-52 (B2/D1'): presupuesto del enlace (bps) = `0,8 × tasa entregada`,
    /// acotado por el tope del usuario. Sin medición todavía, el tope es la
    /// única cota conocida (la política no inventa capacidad).
    fn budget_bps(&self) -> usize {
        let cap = self.max_bps.max(1);
        match self.delivered_bps {
            Some(d) => (((d as f64) * DELIVERY_MARGIN) as usize)
                .min(cap)
                .max(MIN_TARGET_BPS.min(cap)),
            None => cap,
        }
    }

    /// C-52 (B2/D1'): tasa **mínima** de un tamaño (su piso de bpp), pero nunca
    /// por encima de lo que el enlace entrega. Es el nivel en que la escalera
    /// deja de bajar el bitrate y pasa a bajar la escala.
    fn target_min(&self, w: u32, h: u32) -> usize {
        bpp_floor_bps(w, h, self.fps)
            .min(self.budget_bps())
            .max(MIN_TARGET_BPS.min(self.max_bps))
    }

    /// C-52 (B2/D1'): tasa del tamaño **activo** (`target_min` de `self.size`).
    fn floor_bps(&self) -> usize {
        self.target_min(self.size.0, self.size.1)
    }

    /// C-52 (B2): tamaños de la escalera, de mayor a menor. El último es el
    /// suelo (50 % del inicial o [`MIN_LADDER_SIZE`]); los pasos siguen siendo
    /// del 85 % para que un cambio de escala cueste lo mismo que antes.
    ///
    /// Todos son **PARES**: el pipeline 4:2:0 lo exige (`codec/src/conv.rs`) y
    /// la escalera del campo producía 1387x780 / 1001x563 (impares).
    fn ladder_sizes(&self) -> Vec<(u32, u32)> {
        let (fw, fh) = self.ladder_floor_size();
        let mut out = vec![(even(self.initial_size.0), even(self.initial_size.1))];
        let mut cur = out[0];
        while out.len() < 16 {
            let nw = even((cur.0 as f32 * 0.85) as u32).max(320);
            let nh = even((cur.1 as f32 * 0.85) as u32).max(180);
            if nw < fw || nh < fh || (nw, nh) == cur {
                break;
            }
            out.push((nw, nh));
            cur = (nw, nh);
        }
        out
    }

    /// C-52 (B2/D1'): el **mayor** tamaño de la escalera cuyo piso de bpp cabe
    /// en el presupuesto; si ninguno cabe (enlace muy lento), el suelo de
    /// legibilidad — ahí el bitrate baja por debajo del piso (fail-open del
    /// texto) pero el control sigue vivo.
    fn choose_size(&self) -> (u32, u32) {
        let budget = self.budget_bps();
        let rungs = self.ladder_sizes();
        let mut best = *rungs.last().unwrap_or(&self.initial_size);
        for rung in rungs {
            if bpp_floor_bps(rung.0, rung.1, self.fps) <= budget {
                best = rung;
                break; // ya viene de mayor a menor
            }
        }
        best
    }

    /// C-52 (B2/D2'): ¿la histéresis deja cambiar de escala ahora?
    fn scale_hold_expired(&self) -> bool {
        self.now_ms >= self.scale_hold_until_ms
    }

    /// C-52: tamaño mínimo al que puede llegar la escalera — el 50 % del
    /// inicial (C-A2) pero nunca por debajo de [`MIN_LADDER_SIZE`] ni por
    /// encima del propio inicial.
    fn ladder_floor_size(&self) -> (u32, u32) {
        let fw =
            even((self.initial_size.0 as f32 * MIN_SCALE_FRACTION) as u32).max(MIN_LADDER_SIZE.0);
        let fh =
            even((self.initial_size.1 as f32 * MIN_SCALE_FRACTION) as u32).max(MIN_LADDER_SIZE.1);
        (fw.min(self.initial_size.0), fh.min(self.initial_size.1))
    }

    /// C-52 (B2): aplica un cambio de escala. `raising` decide la tasa del
    /// tamaño nuevo: al subir se exige su piso de bpp (texto legible); al bajar
    /// se conserva la tasa vigente si ya era menor (nunca se SUBE la tasa en un
    /// escalón de bajada, que sería absurdo bajo congestión).
    fn apply_scale(
        &mut self,
        size: (u32, u32),
        reason: &'static str,
        raising: bool,
    ) -> Option<AdaptDecision> {
        if size == self.size {
            return None;
        }
        let min = self.target_min(size.0, size.1);
        self.size = size;
        self.bps = if raising { min } else { self.bps.min(min) };
        // D2': ningún otro cambio de escala durante la histéresis.
        self.scale_hold_until_ms = self.now_ms + SCALE_HYSTERESIS_MS;
        // La subida siguiente exige muestras NUEVAS (3 buenas, D2).
        self.net_low = 0;
        Some(AdaptDecision {
            video_size: Some(size),
            target_bps: None,
            reason,
        })
    }

    /// C-52 (D1): un escalón de escala hacia abajo (85 %), con el piso de bpp
    /// del tamaño nuevo. `None` en el suelo de legibilidad: ahí solo cabe
    /// esperar la recuperación.
    fn scale_down(&mut self, reason: &'static str) -> Option<AdaptDecision> {
        if !self.scale_hold_expired() {
            return None;
        }
        let rungs = self.ladder_sizes();
        let next = match rungs.iter().position(|r| *r == self.size) {
            Some(i) => *rungs.get(i + 1)?,
            // Tamaño fuera de la escalera (estado inyectado): un paso del 85 %.
            None => {
                let (w0, h0) = self.size;
                let nw = even((w0 as f32 * 0.85) as u32).max(320);
                let nh = even((h0 as f32 * 0.85) as u32).max(180);
                let (fw, fh) = self.ladder_floor_size();
                if nw < fw || nh < fh || (nw, nh) == self.size {
                    return None; // suelo de legibilidad: solo bitrate/recuperación
                }
                (nw, nh)
            }
        };
        self.apply_scale(next, reason, false)
    }

    /// C-52 (B2): un escalón de **subida**. Primero el mayor tamaño que cabe en
    /// el presupuesto (un solo salto: subir de 85 % en 85 % con histéresis de
    /// 10 s tardaría un minuto); si ya está, sube la tasa hasta el presupuesto,
    /// nunca hasta el tope del usuario a ciegas.
    fn recover(
        &mut self,
        scale_reason: &'static str,
        rate_reason: &'static str,
    ) -> Option<AdaptDecision> {
        let want = self.choose_size();
        if want != self.size {
            if !self.scale_hold_expired() {
                // Sin margen para tocar la escala: se queda como está (subir el
                // bitrate de un tamaño que no cabe reintroduce la saturación).
                return None;
            }
            return self.apply_scale(want, scale_reason, true);
        }
        let top = self.budget_bps();
        if self.bps < top {
            self.bps = (self.bps * 3 / 2).min(top);
            return Some(AdaptDecision {
                video_size: None,
                target_bps: Some(self.bps),
                reason: rate_reason,
            });
        }
        None
    }

    /// C-52 (B2/D1'): entra una medición de la tasa que el enlace **entrega**
    /// (bps, del [`DeliveryMeter`], no el objetivo del encoder).
    ///
    /// Es la corrección del defecto P1 del campo: el objetivo es
    /// `min(piso_bpp(tamaño), 0,8 × entregada)` y, si el piso no cabe, se baja
    /// la **resolución** — nunca se pide más bitrate del que hay. Bajar no
    /// espera a la histéresis (es la protección del control); subir sí.
    pub fn observe_delivery(&mut self, delivered_bps: usize) -> Option<AdaptDecision> {
        if delivered_bps == 0 {
            return None;
        }
        self.delivered_bps = Some(delivered_bps);
        // 1) Techo duro: el objetivo nunca supera el presupuesto.
        let budget = self.budget_bps();
        let mut rate_dec = None;
        if self.bps > budget {
            self.bps = budget;
            rate_dec = Some(AdaptDecision {
                video_size: None,
                target_bps: Some(budget),
                reason: "presupuesto_baja",
            });
        }
        // 2) Tamaño: el mayor cuyo piso de bpp cabe en el presupuesto.
        let want = self.choose_size();
        if want != self.size && self.scale_hold_expired() {
            let raising = want.0 * want.1 > self.size.0 * self.size.1;
            let reason = if raising {
                "presupuesto_escala_sube"
            } else {
                "presupuesto_escala_baja"
            };
            if let Some(dec) = self.apply_scale(want, reason, raising) {
                return Some(dec);
            }
        }
        if rate_dec.is_some() {
            return rate_dec;
        }
        // 3) Legibilidad del tamaño activo: si la tasa quedó por debajo de su
        // piso de bpp (p. ej. tras un escalón que arrastró una tasa baja), se
        // sube a ese piso — siempre ≤ presupuesto.
        let min = self.floor_bps();
        if self.bps < min {
            self.bps = min;
            return Some(AdaptDecision {
                video_size: None,
                target_bps: Some(min),
                reason: "presupuesto_sube",
            });
        }
        None
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
        // C-52 (B2/D2'): una decisión de CPU cubre ~60 frames (4 s a 15 fps):
        // el reloj de la política avanza también por aquí, o en una sesión sin
        // muestras de RTT la histéresis de escala nunca expiraría.
        self.now_ms += 60.0 * 1000.0 / self.fps as f64;

        if self.ema_ms > 0.9 * budget_ms {
            // Presión (C-52): el bitrate no baja del piso de bpp del tamaño
            // activo; si ya está en él, se baja la ESCALA (menos píxeles a la
            // misma calidad por píxel) en vez de hundir la tasa.
            let floor = self.floor_bps();
            if self.bps > floor {
                self.bps = (self.bps / 2).max(floor).min(self.budget_bps());
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
            // Sobra capacidad: primero escala, luego bitrate (la escalera
            // probada), siempre acotado por lo que el enlace entrega.
            return self.recover("recupera_escala", "recupera_bitrate");
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
    /// decidir una bajada (ventana nueva); `net_low` cuenta muestras
    /// sanas **y neutras** — es lo que hace alcanzable la recuperación con el
    /// jitter del relay (C-52/D2).
    ///
    /// C-52 (B2/D2'): cada llamada es 1 s del reloj de la política (el
    /// controlador publica el RTT a 1/s) y alimenta la histéresis de escala.
    pub fn observe_net(&mut self, rtt_ms: u32) -> Option<AdaptDecision> {
        self.now_ms += 1000.0;
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
            // C-52 (B2/D1'): el bitrate baja, pero nunca por debajo del piso de
            // bpp del tamaño activo ni por encima de lo que el enlace entrega;
            // si ya está en ese nivel, baja la ESCALA.
            let floor = self.floor_bps();
            if self.bps > floor {
                self.bps = (self.bps / 2).max(floor).min(self.budget_bps());
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
            // C-52 (B2/D2'): la ventana de SUBIDA se reabre (3 muestras nuevas)
            // pero la evidencia de congestión (`net_high`) NO se borra: era
            // justo lo que faltaba para que un pico posterior volviera a bajar
            // el escalón recién recuperado sin confirmación.
            let dec = self.recover("red_recupera_escala", "red_recupera_bitrate");
            if dec.is_some() {
                self.net_low = 0;
                self.net_congested = false;
            }
            return dec;
        }
        None
    }
}

/// C-52 (D3): ¿este escalón **sube** la tasa (recuperación)? Solo entonces hay
/// que refrescar: bajar la tasa no devuelve detalle y un IDR extra costaría un
/// pico de bitrate justo cuando la red ya va mal.
fn decision_needs_refresh(reason: &str) -> bool {
    matches!(
        reason,
        "recupera_bitrate" | "red_recupera_bitrate" | "presupuesto_sube"
    )
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

/// C-52 (B2/D1'): mide la **capacidad** que el enlace entrega de verdad, con la
/// escritura de cada frame de vídeo al transporte — no con el objetivo del
/// encoder. Es la magnitud que faltaba en campo: el encoder pedía 14–25 Mbps y
/// el socket entregaba ~1 Mbps sin que nadie lo supiera (`entregados=0` en 3 de
/// 6 bloques de `evidence/c52-field-gate-failed.md` §2.2).
///
/// **Solo una escritura BLOQUEANTE mide el enlace** (`elapsed >=
/// VIDEO_WRITE_BUDGET_MS`): el buffer del socket estaba lleno y el tiempo es el
/// drenaje real. Una escritura instantánea no dice nada de la capacidad —solo
/// que ese frame cabía— y usarla como techo crearía un lazo cerrado perverso:
/// 0,8 × lo que ofrecemos recorta el objetivo un 20 % en cada ventana hasta
/// hundirlo. Por eso, sin saturación, la capacidad es **desconocida** y manda
/// el tope del usuario.
#[derive(Debug, Default)]
pub struct DeliveryMeter {
    /// Capacidad medida (bps). Baja con cada medida nueva; nunca sube (subir
    /// es lo que satura y hace oscilar la escalera).
    capacity_bps: Option<f64>,
    /// Último valor publicado (para no inundar de decisiones/eventos con
    /// muestras que cambian poco).
    published_bps: Option<f64>,
}

impl DeliveryMeter {
    /// Caída relativa mínima para publicar una capacidad nueva. Por debajo, la
    /// medida es la misma a efectos de política y solo generaría ruido
    /// (`video_adapt` y `set_target_bitrate` por frame).
    const MATERIAL_DROP: f64 = 0.9;

    /// Registra un frame de vídeo entregado al transporte. Devuelve la
    /// capacidad medida (bps) cuando la escritura reveló el drenaje del enlace
    /// **y** el cambio es material.
    pub fn observe(&mut self, bytes: usize, elapsed_ms: f64) -> Option<usize> {
        if bytes == 0 || elapsed_ms < VIDEO_WRITE_BUDGET_MS {
            return None;
        }
        let sample = bytes as f64 * 8.0 * 1000.0 / elapsed_ms.max(1.0);
        self.capacity_bps = Some(match self.capacity_bps {
            Some(prev) => prev.min(sample),
            None => sample,
        });
        let now = self.capacity_bps.unwrap_or(0.0);
        let material = match self.published_bps {
            None => true,
            Some(prev) => now <= prev * Self::MATERIAL_DROP,
        };
        if material {
            self.published_bps = Some(now);
            self.bps()
        } else {
            None
        }
    }

    /// Capacidad medida que conoce la política (bps). Es el último valor
    /// **publicado**, no cada muestra: lo que la política ve es estable.
    pub fn bps(&self) -> Option<usize> {
        self.published_bps.map(|v| v as usize)
    }
}

/// C-52 (B2/P1): compuerta de frames de vídeo del camino TCP.
///
/// En TCP el vídeo y el control **comparten socket**: una escritura que tarda
/// más de [`VIDEO_WRITE_BUDGET_MS`] significa que el buffer está lleno y que la
/// entrada del controlador queda **encolada detrás del vídeo** (el campo del
/// 23/11: «no se puede seleccionar ni mover una ventana»). La compuerta deja de
/// alimentar vídeo durante [`VIDEO_DROP_HOLD_MS`] para que el socket drene; el
/// visor prefiere frames frescos. **Las tramas de control (input, audio,
/// latidos) no pasan por aquí: nunca se descartan.**
#[derive(Debug, Default)]
pub struct VideoGate {
    hold_until_ms: f64,
    /// Frames descartados en el episodio vigente (para informarlos una vez).
    dropped: u32,
}

impl VideoGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// ¿Se puede escribir un frame de vídeo ahora? `false` = descártalo.
    pub fn allows(&self, now_ms: f64) -> bool {
        now_ms >= self.hold_until_ms
    }

    /// El frame se escribió en `elapsed_ms`: si el socket estaba lleno, se
    /// cierra la compuerta una temporada.
    pub fn on_written(&mut self, now_ms: f64, elapsed_ms: f64) {
        if elapsed_ms >= VIDEO_WRITE_BUDGET_MS {
            self.hold_until_ms = now_ms + VIDEO_DROP_HOLD_MS;
        }
    }

    /// Un frame se descartó por la compuerta.
    pub fn on_dropped(&mut self) {
        self.dropped = self.dropped.saturating_add(1);
    }

    /// Si el hold ya expiró y hubo descartes, cierra el episodio y devuelve
    /// cuántos frames se descartaron (para el evento `video_dropped_slow`).
    pub fn close_episode(&mut self, now_ms: f64) -> Option<u32> {
        if now_ms >= self.hold_until_ms && self.dropped > 0 {
            let n = self.dropped;
            self.dropped = 0;
            return Some(n);
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
    // C-52 (B2/D1'): tasa que el enlace ENTREGA (bps; 0 = aún sin medición).
    mut delivery_rx: tokio::sync::watch::Receiver<u64>,
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
                    "[video] fps ({} s): captura={} entraron={} emitidos={} entregados={} · captura_ms={:.1} escala_ms={:.1} encode_ms={:.1} · {:.0} kbps {:.1} KB/frame {:.3} bpp ({width}x{height}) · objetivo={target_kbps} kbps entregada={delivered_kbps} kbps",
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
                    // C-52 (B2/D1'): el objetivo y lo que el enlace entrega
                    // (en el mismo renglón que el bpp: el gate de campo compara
                    // ambos y con 0.14.4 solo se veía el primero).
                    target_kbps = adapt.as_ref().map(|s| s.bps / 1000).unwrap_or(0),
                    delivered_kbps = adapt
                        .as_ref()
                        .and_then(|s| s.delivered_bps)
                        .map(|d| d / 1000)
                        .unwrap_or(0),
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

            // Control adaptativo de calidad: presupuesto de ENTREGA (C-52 B2,
            // manda sobre todo lo demás), pista de RED (C-13, la observa el
            // controlador 1/s) o la observación de encode cada ~60 frames.
            let mut net_rtt = None;
            if net_rx.has_changed().unwrap_or(false) {
                net_rtt = Some(*net_rx.borrow_and_update());
            }
            let mut delivered = None;
            if delivery_rx.has_changed().unwrap_or(false) {
                let v = *delivery_rx.borrow_and_update();
                if v > 0 {
                    delivered = Some(v as usize);
                }
            }
            if let Some(st) = adapt.as_mut() {
                let dec = delivered
                    .and_then(|d| st.observe_delivery(d))
                    .or_else(|| net_rtt.and_then(|rtt| st.observe_net(rtt)))
                    .or_else(|| st.observe(took_ms, budget_ms));
                if let Some(dec) = dec {
                    if let Some(bps) = dec.target_bps {
                        // C-52 (D3): al recuperar la tasa se pide un IDR para
                        // que el contenido estático se recodifique con la
                        // calidad nueva.
                        if apply_target_bps(encoder.as_mut(), bps, dec.reason) {
                            let _ = out.try_send(VideoMsg::Event(json!({
                                "event": "video_adapt", "reason": dec.reason, "bps": bps,
                                "size": format!("{}x{}", st.size.0, st.size.1),
                                // C-40: RTT que disparó la decisión (sin él no
                                // se distingue congestión real de un pico).
                                "rtt": net_rtt,
                                // C-52 (B2/D1'): presupuesto vigente — en campo
                                // se ve si el objetivo se adaptó al enlace.
                                "delivered": st.delivered_bps,
                                "budget": st.budget_bps(),
                            })));
                        }
                    }
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
                            "delivered": adapt.as_ref().and_then(|s| s.delivered_bps),
                        })));
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
        // C-52 (B2/D1'): con el tope (3 M) por debajo del piso de bpp de 720p,
        // la tasa ya nace acotada por el tope; arranca además en el valor
        // conservador (2 M) y sube con evidencia.
        assert_eq!(s.bps, 2_000_000, "tasa inicial conservadora, no el piso");
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
        // C-52 (B2/D1'): recupera hasta el mayor tamaño cuyo piso de bpp cabe
        // en el presupuesto (tope 3 Mbps) — no al inicial a ciegas.
        assert_eq!(
            s.size,
            (784, 442),
            "el mayor que cabe en 3 Mbps (par, 4:2:0)"
        );
    }

    /// C-A2: la escalera no baja del 50 % del inicial y el bitrate no se
    /// reinfla en cada escalón (campo 15/11: 6 escalas en 2 min sin
    /// recuperación → 850x478@200k, texto ilegible).
    ///
    /// C-52 (B2/D1'): el «reinflado» se prohíbe de raíz — un escalón de bajada
    /// nunca SUBE la tasa (antes cada escalón volvía a pedir `w*h`), y la tasa
    /// jamás pasa de lo que se declara presupuesto.
    #[test]
    fn scale_ladder_is_capped_and_does_not_reinflate() {
        let mut s = AdaptState::new((1920, 1080), Some(6_000_000));
        let mut escalas = 0;
        let mut prev = s.bps;
        for _ in 0..150 {
            if let Some(d) = s.observe_net(500) {
                if d.reason == "red_escala" {
                    escalas += 1;
                    let (w, h) = s.size;
                    assert!(s.bps <= prev, "el escalón no reinfla: {} > {prev}", s.bps);
                    assert!(
                        s.bps <= bpp_floor_bps(w, h, 15).min(s.max_bps),
                        "escalón acotado por el piso de {w}x{h}: {}",
                        s.bps
                    );
                }
                assert!(s.bps <= s.max_bps, "respeta el tope: {}", s.bps);
                prev = s.bps;
            }
        }
        assert!(escalas > 0 && escalas <= 6, "escalera acotada: {escalas}");
        assert!(
            s.size.0 >= 960 && s.size.1 >= 540,
            "nunca por debajo del 50 %: {:?}",
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
        assert_eq!(
            start, 2_000_000,
            "C-52 (B2): arranque conservador, no el piso a ciegas"
        );
        // Confirmación: 1 y 2 muestras altas → nada aún.
        assert_eq!(s.observe_net(300), None, "1ª muestra");
        assert_eq!(s.observe_net(300), None, "2ª muestra");
        // 3ª → en el piso de bpp del tamaño activo, baja ESCALA (C-52: el
        // bitrate solo baja hasta el piso; por debajo miente).
        let d = s.observe_net(300).expect("3ª muestra → decisión");
        assert_eq!(d.reason, "red_escala");
        assert_eq!(s.bps, start, "el bitrate no se hunde");
        assert!(d.video_size.is_some());
        assert!(s.bps <= s.budget_bps(), "nunca por encima del presupuesto");
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
        // Sano sostenido + cooldown agotado → recupera (primero la escala,
        // luego el bitrate: la escalera probada).
        let mut got = None;
        for _ in 0..15 {
            got = got.or_else(|| s.observe_net(0));
        }
        let d = got.expect("recupera tras cooldown");
        assert_eq!(d.reason, "red_recupera_escala");
        // C-52 (B2/D1'): no se vuelve al tamaño inicial a ciegas: el techo de
        // 3 Mbps no sostiene su piso de bpp (6,2 Mbps), así que la recuperación
        // sube al mayor tamaño que SÍ cabe en el presupuesto.
        assert_eq!(
            s.size,
            (784, 442),
            "el mayor que cabe en 3 Mbps (par, 4:2:0)"
        );
        assert!(
            bpp_floor_bps(s.size.0, s.size.1, 15) <= 3_000_000,
            "el piso del tamaño recuperado cabe en el presupuesto"
        );
        assert!(
            bpp_floor_bps(924, 520, 15) > 3_000_000,
            "y el siguiente escalón ya no cabe (por eso para ahí)"
        );
        assert!(
            s.bps <= 3_000_000,
            "y el bitrate no pasa del tope: {}",
            s.bps
        );
        assert!(!s.net_congested, "recuperación desbloquea la CPU");
    }

    #[test]
    fn net_congestion_at_floor_scales_down_and_blocks_cpu_recovery() {
        // Tope por encima del piso de 960x540@15 (3,5 Mbps) para que la
        // escalera tenga margen: el suelo de bpp no es un número de kbps.
        let mut s = AdaptState::new((960, 540), Some(6_000_000));
        assert_eq!(
            s.bps, 2_000_000,
            "C-52 (B2/D1'): arranque conservador; el piso (3,5 M) se alcanza con evidencia"
        );
        assert_eq!(s.floor_bps(), 3_499_200, "piso de bpp de 960x540@15");
        let floor_size = s.size;
        assert_eq!(s.observe_net(500), None);
        assert_eq!(s.observe_net(500), None);
        let d = s.observe_net(500).expect("en el piso de bpp → escala");
        assert_eq!(d.reason, "red_escala");
        assert!(d.video_size.is_some());
        assert!(s.size != s.initial_size);
        assert_eq!(s.size, (816, 458), "un escalón del 85 % (par, 4:2:0)");
        assert!(
            s.bps <= s.floor_bps(),
            "el tamaño nuevo fija su piso (o menos): {} <= {}",
            s.bps,
            s.floor_bps()
        );
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
        // Ahora sí: con el bitrate POR DEBAJO del techo, la CPU vuelve a
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
        // C-52 (B2/D1'): sin tope, la tasa inicial ya NO es el piso de bpp a
        // ciegas (13,9 Mbps a 1080p@15 sobre un enlace de 1 Mbps = el defecto de
        // campo del 23/11): arranca en `INITIAL_TARGET_BPS` y sube con
        // evidencia de entrega.
        let free = AdaptState::new((1920, 1080), None);
        let base = (1920 * 1080 * 3) as usize;
        assert_eq!(
            bpp_floor_bps(1920, 1080, 15),
            13_996_800,
            "el piso de bpp sigue siendo la referencia de legibilidad"
        );
        assert_eq!(free.bps, 2_000_000, "tasa inicial conservadora");
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
        let d = saw.expect("con CPU holgada hay recuperación");
        assert_eq!(d.reason, "recupera_bitrate");
        assert!(d.target_bps.unwrap() <= 500_000, "no pasa del tope");
    }

    #[test]
    fn adapt_low_bitrate_first_then_scale_under_pressure() {
        let mut s = AdaptState::new((960, 540), None);
        let budget = 66.0;
        // C-52: «bitrate primero» solo aplica cuando una recuperación ya subió
        // la tasa (estado real tras un escalón de subida). Se simula ese punto.
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
        assert!(s.bps <= s.floor_bps(), "el tamaño nuevo fija su piso");
        // Con holgura: recupera la escala inicial (cabe en el presupuesto, que
        // sin medición es el techo) tras agotar la histéresis de 10 s.
        let mut recovered = None;
        for _ in 0..(60 * 12) {
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

    /// C-52 (B2/D1'): la escalera ya **no arranca** pidiendo el piso de bpp a
    /// ciegas (13 996 800 = 0,450 bpp a 1080p@15; en el enlace de ~1 Mbps del
    /// campo del 23/11 eso eran 14 s de vídeo encolado delante del control).
    /// Arranca conservadora y sube al piso cuando la ENTREGA lo permite.
    #[test]
    fn field_ladder_starts_conservative_and_climbs_with_delivery() {
        let st = AdaptState::with_fps((1920, 1080), Some(15_000_000), FIELD_FPS);
        assert_eq!(st.bps, INITIAL_TARGET_BPS, "arranque conservador");
        assert!(
            bpp_of(st.bps, 1920, 1080, FIELD_FPS) < TEXT_BPP_FLOOR,
            "el arranque no pretende el piso: no hay evidencia del enlace"
        );
        // Escalones de campo sobre `w*h*3` = 6 220 800 (0,200 bpp): ilegibles.
        assert!(
            bpp_of(6_220_800, 1920, 1080, FIELD_FPS) < TEXT_BPP_FLOOR,
            "la tasa inicial del campo (w*h*3) era ilegible: 0,200 bpp"
        );

        // Enlace de ~1 Mbps (el del gate de campo fallido): el presupuesto
        // (0,8 Mbps) no sostiene ni el suelo de la escalera → se queda en el
        // suelo y el objetivo es lo que HAY (fail-open del texto, control vivo).
        let mut slow = AdaptState::with_fps((1920, 1080), Some(15_000_000), FIELD_FPS);
        assert!(slow.observe_delivery(1_000_000).is_some());
        assert_eq!(slow.bps, 800_000, "0,8 × 1 Mbps entregados");
        assert_eq!(
            slow.size,
            (1000, 562),
            "el suelo de la escalera (par, 4:2:0)"
        );

        // Enlace que sí entrega (20 Mbps): el objetivo llega al piso de bpp y
        // las muestras sanas lo suben al tope CLI (`--max-bitrate 15000`).
        let mut fast = AdaptState::with_fps((1920, 1080), Some(15_000_000), FIELD_FPS);
        fast.observe_delivery(20_000_000);
        assert_eq!(fast.bps, 13_996_800, "objetivo = piso de bpp (cabe)");
        let mut ladder: Vec<(&str, usize)> = Vec::new();
        // 3 muestras sanas (exceso ≤100 ms) → 1 recuperación (NET_RECOVER=3).
        for _ in 0..15 {
            if let Some(d) = fast.observe_net(0) {
                ladder.push((d.reason, d.target_bps.expect("decisión de tasa")));
            }
        }
        assert_eq!(
            ladder,
            vec![("red_recupera_bitrate", 15_000_000)],
            "del piso de bpp al tope CLI sin pasar del presupuesto"
        );
    }

    /// **Gate D1** `ladder_preserves_bpp`: con un enlace que **sostiene** el
    /// piso de bpp (se le informa una entrega de 20 Mbps → presupuesto acotado
    /// por el tope de 15 Mbps), ningún escalón —de arranque, de escala, bajo
    /// congestión o bajo presión de CPU— fija una tasa por debajo de 0,45 bpp
    /// del tamaño activo. Si el enlace NO sostiene el piso, la política baja la
    /// escala y el bitrate se queda en el presupuesto (nunca por encima).
    #[test]
    fn ladder_preserves_bpp() {
        let cap = 15_000_000usize;
        let mut st = AdaptState::with_fps((1920, 1080), Some(cap), FIELD_FPS);
        // Evidencia de entrega: 20 Mbps → el piso de 1080p cabe en el tope.
        assert!(st.observe_delivery(20_000_000).is_some());
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
        // Suelo: el mismo 50 % del inicial que en campo (1001x563), con el piso
        // de bpp en vez de 200 kbps → 3,8 Mbps (0,450 bpp) en lugar de 0,003.
        assert_eq!(st.size, (1000, 562), "suelo de escala (50 % del inicial)");
        assert!(
            st.bps >= bpp_floor_bps(1000, 562, FIELD_FPS),
            "el suelo es de bpp, no de kbps: {} bps",
            st.bps
        );
        // Enlace de 1 Mbps (campo 23/11): el piso de 1080p no cabe → baja la
        // escala y el objetivo queda EXACTAMENTE en el presupuesto. Nunca se
        // pide más bitrate del que el enlace entrega.
        let mut slow = AdaptState::with_fps((1920, 1080), Some(15_000_000), FIELD_FPS);
        for _ in 0..90 {
            slow.observe_delivery(1_000_000);
            slow.observe_net(500);
            assert!(slow.bps <= 800_000, "respeta la entrega: {}", slow.bps);
        }
        assert_eq!(
            slow.size,
            (1000, 562),
            "se queda en el suelo de la escalera (par, 4:2:0)"
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
        assert_eq!(
            st.size,
            (1000, 562),
            "mismo suelo de escala que en campo, redondeado a par (4:2:0)"
        );
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

    /// **Gate D1'** `video_target_never_exceeds_delivered_rate`: el objetivo de
    /// vídeo **nunca** supera el 80 % de lo que el enlace ENTREGA (medido, no
    /// el objetivo del encoder). Se reproduce el gate de campo fallido del
    /// 23/11 (`evidence/c52-field-gate-failed.md` §2.2): bloques de 5 s a
    /// **2802 / 507 / 3134 / 383 kbps** y ~1 Mbps de sesión, con las tres
    /// resoluciones probadas (720p, 1080p y **nativa 4K**).
    #[test]
    fn video_target_never_exceeds_delivered_rate() {
        /// Entregado real de los bloques `[video] fps (5 s)` del anfitrión.
        const FIELD_DELIVERED_BPS: &[usize] = &[2_802_000, 507_000, 3_134_000, 383_000, 1_000_000];
        for size in [(1280, 720), (1920, 1080), (3840, 2160)] {
            // Tope de 60 Mbps: el piso de 4K (56 Mbps) cabría en el papel — así
            // el gate prueba que lo que manda es la ENTREGA, no el piso.
            let mut st = AdaptState::with_fps(size, Some(60_000_000), FIELD_FPS);
            for &delivered in FIELD_DELIVERED_BPS {
                if let Some(d) = st.observe_delivery(delivered) {
                    st.observe_net(0); // el controlador publica el RTT 1/s
                    if let Some(bps) = d.target_bps {
                        assert!(bps <= st.bps, "tasa pedida {bps} > vigente {}", st.bps);
                    }
                }
                let budget = (delivered as f64 * DELIVERY_MARGIN) as usize;
                assert!(
                    st.bps <= budget.max(MIN_TARGET_BPS),
                    "objetivo {} > 0,8 × {delivered} entregados ({size:?})",
                    st.bps
                );
                assert!(
                    st.bps <= st.budget_bps(),
                    "el objetivo nunca pasa del presupuesto: {} > {}",
                    st.bps,
                    st.budget_bps()
                );
            }
            // El tamaño elegido es el mayor cuyo piso de bpp cabe en el
            // presupuesto; con ~1 Mbps ningún escalón de 1080p/4K cabe → suelo.
            assert_eq!(
                st.size,
                *st.ladder_sizes().last().expect("escalera no vacía"),
                "suelo de la escalera"
            );
        }

        // El bloque de sesión (~1 Mbps): objetivo 800 kbps. En la Fase B el
        // mismo enlace pedía 13,8 Mbps a 1080p y **56 Mbps** en nativa 4K.
        for size in [(1280, 720), (1920, 1080), (3840, 2160)] {
            let mut st = AdaptState::with_fps(size, Some(60_000_000), FIELD_FPS);
            st.observe_delivery(1_000_000);
            assert_eq!(st.bps, 800_000, "0,8 × 1 Mbps entregados ({size:?})");
            assert!(
                st.bps < bpp_floor_bps(size.0, size.1, FIELD_FPS),
                "con ese enlace el objetivo queda por debajo del piso de bpp \
                 ({size:?}): fail-open del texto, no saturación"
            );
        }

        // Contraste: con un enlace que sí entrega (20 Mbps) el objetivo llega
        // al piso de bpp de 1080p (13 996 800) — el presupuesto no recorta.
        let mut fast = AdaptState::with_fps((1920, 1080), Some(60_000_000), FIELD_FPS);
        fast.observe_delivery(20_000_000);
        assert_eq!(fast.bps, 13_996_800, "el piso cabe en 20 Mbps entregados");
        assert_eq!(
            fast.size,
            (1920, 1080),
            "y se queda en la resolución pedida"
        );
    }

    /// **Gate D2'** `ladder_does_not_oscillate`: la escalera **no** puede
    /// cambiar de escala más de una vez cada 10 s con un enlace que oscila (el
    /// campo del 23/11 alternó `bpp 0,477 → 0,112 → 0,610 → 0,496 → 0,121 →
    /// 0,734` en 30 s), y la subida exige 3 muestras buenas nuevas.
    #[test]
    fn ladder_does_not_oscillate() {
        // 1) Con los números REALES del campo (2802/507/3134/383 kbps) el
        //    presupuesto ni siquiera permite un escalón: el tamaño se queda en
        //    el suelo y no hay nada que oscilar.
        const FIELD_DELIVERED_BPS: &[usize] = &[2_802_000, 507_000, 3_134_000, 383_000];
        let mut field = AdaptState::with_fps((1920, 1080), Some(60_000_000), FIELD_FPS);
        let mut field_changes = 0;
        for _ in 0..8 {
            for &delivered in FIELD_DELIVERED_BPS {
                if let Some(d) = field.observe_delivery(delivered) {
                    if d.video_size.is_some() {
                        field_changes += 1;
                    }
                }
                field.observe_net(150);
            }
        }
        assert!(
            field_changes <= 1,
            "con los números de campo no hay oscilación: {field_changes} cambios"
        );

        // 2) Enlace que oscila de verdad (cruza el escalón de 1178x663): sin
        //    histéresis cambiaría de tamaño cada segundo.
        let mut st = AdaptState::with_fps((1920, 1080), Some(60_000_000), FIELD_FPS);
        let mut changes: Vec<usize> = Vec::new();
        for sec in 0..30usize {
            let delivered = if sec % 2 == 0 { 10_000_000 } else { 1_000_000 };
            if let Some(d) = st.observe_delivery(delivered) {
                if d.video_size.is_some() {
                    changes.push(sec);
                }
            }
            if let Some(d) = st.observe_net(150) {
                if d.video_size.is_some() {
                    changes.push(sec);
                }
            }
        }
        assert!(
            !changes.is_empty(),
            "la escalera reacciona al enlace (si no, el gate no prueba nada)"
        );
        // Cota dura: un cambio como mucho cada 10 s → 1 + 30/10 en 30 s. El
        // campo del 23/11 hizo 6 en 30 s (sin histéresis).
        assert!(
            changes.len() <= 4,
            "{} cambios de escala en 30 s (campo: 6): {changes:?}",
            changes.len()
        );
        for pair in changes.windows(2) {
            let gap_s = (pair[1] - pair[0]) as f64;
            // Granularidad de 1 s: cada muestra de RTT avanza el reloj de la
            // política exactamente 1 s, así que un hueco de 9 s de reloj de
            // pared son 10 s de política.
            assert!(
                gap_s * 1000.0 + 1000.0 >= SCALE_HYSTERESIS_MS,
                "dos cambios de escala a {gap_s} s (< {} s): {changes:?}",
                SCALE_HYSTERESIS_MS / 1000.0
            );
        }

        // 3) La subida no es inmediata: hacen falta `NET_RECOVER` muestras
        //    buenas nuevas (no una sola).
        let mut st = AdaptState::with_fps((1920, 1080), Some(15_000_000), FIELD_FPS);
        st.size = (1001, 563);
        st.bps = 900_000;
        let mut ups = 0;
        for i in 1..=NET_RECOVER {
            let dec = st.observe_net(0);
            if i < NET_RECOVER {
                assert!(dec.is_none(), "no sube con {i} muestra(s) buena(s)");
            } else if dec.is_some() {
                ups += 1;
            }
        }
        assert_eq!(ups, 1, "3 muestras buenas → un escalón de subida");
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
        // Estado de una sesión que ya había subido (la tasa de arranque es
        // conservadora desde B2; el refresco se prueba en la recuperación).
        st.bps = 14_000_000;
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

/// C-52 (B2/P1) · arnés del canal TCP compartido (SOLO tests).
///
/// En TCP el vídeo y el control van por el MISMO socket: lo que el vídeo deja
/// encolado lo espera la entrada del controlador. El campo del 23/11 lo midió
/// como «no se puede seleccionar ni mover una ventana» con ~1 Mbps entregados y
/// frames de 131,5 KB (2 MB/s ofrecidos sobre 1 Mbit/s). Este arnés modela el
/// bucle de sesión con la política REAL ([`VideoGate`] + [`DeliveryMeter`] +
/// [`AdaptState`]) y comprueba quién paga la espera.
#[cfg(test)]
mod c52_tcp {
    use super::*;

    /// Enlace del gate de campo: ~1 Mbps (15,5 MB en ~2 min).
    const LINK_BPS: f64 = 1_000_000.0;
    /// Bytes por milisegundo que drena ese enlace (125 B/ms = 1 Mbit/s).
    const LINK_BYTES_PER_MS: f64 = LINK_BPS / 8000.0;
    /// Frame del anfitrión en campo con el encoder pidiendo el piso de bpp a
    /// ciegas (`[video] fps (5 s)` §2.2 de `c52-field-gate-failed.md`).
    const FIELD_FRAME_BYTES: f64 = 131_500.0;
    /// Cadencia del encoder (`--video-fps 15`).
    const FRAME_MS: f64 = 1000.0 / 15.0;
    /// Entrada del controlador: 20 eventos/s de 48 B (ratón).
    const CONTROL_MS: f64 = 50.0;
    const CONTROL_BYTES: f64 = 48.0;
    /// Cola del socket: **2 frames de vídeo en vuelo**, el máximo del diseño
    /// (C-52/B2, P1). El buffer arranca VACÍO en cada simulación.
    const IN_FLIGHT_FRAMES: f64 = 2.0;
    /// Ventana simulada (12 s: tres bloques de 5 s del log de campo).
    const RUN_MS: f64 = 12_000.0;

    /// Socket FIFO de capacidad finita (`cap`) que drena a la tasa del enlace.
    /// Modela lo que el emisor ve: `write_all` vuelve cuando el último byte
    /// entra en el buffer (bloqueando si está lleno) y el byte sale por el
    /// cable a la tasa del enlace.
    #[derive(Default)]
    struct Socket {
        queued_bytes: f64,
        queued_at_ms: f64,
    }

    impl Socket {
        fn advance(&mut self, t_ms: f64) {
            let dt = (t_ms - self.queued_at_ms).max(0.0);
            self.queued_bytes = (self.queued_bytes - LINK_BYTES_PER_MS * dt).max(0.0);
            self.queued_at_ms = t_ms;
        }

        /// Encola `bytes` en `t_ms` con capacidad `cap`.
        /// Devuelve (bloqueo del emisor, instante de entrega del último byte).
        fn write(&mut self, t_ms: f64, bytes: f64, cap: f64) -> (f64, f64) {
            self.advance(t_ms);
            let block_ms = ((self.queued_bytes + bytes - cap).max(0.0)) / LINK_BYTES_PER_MS;
            let delivery_ms = t_ms + (self.queued_bytes + bytes) / LINK_BYTES_PER_MS;
            self.queued_bytes = (self.queued_bytes + bytes).min(cap.max(bytes));
            self.queued_at_ms = t_ms + block_ms;
            (block_ms, delivery_ms)
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Policy {
        /// C-52 Fase B2: compuerta + presupuesto de entrega (producción).
        B2,
        /// C-52 Fase B (el 0.14.4): encoder al piso de bpp a ciegas (131,5 KB
        /// por frame) y **sin** compuerta: todo el vídeo se encola.
        FaseB,
    }

    #[derive(Default, Debug)]
    struct Stats {
        frames_attempted: u32,
        frames_written: u32,
        frames_dropped: u32,
        controls: u32,
        /// Cambios de escala del presupuesto (anti-oscilación).
        scale_changes: u32,
        /// Tiempo que el bucle estuvo BLOQUEADO escribiendo vídeo.
        blocked_ms: f64,
        /// Duración total de la simulación (ms).
        total_ms: f64,
        /// Latencia (ms) de entrega de cada evento de control.
        latencies: Vec<f64>,
    }

    impl Stats {
        fn mean_latency(&self) -> f64 {
            if self.latencies.is_empty() {
                return 0.0;
            }
            self.latencies.iter().sum::<f64>() / self.latencies.len() as f64
        }

        fn max_latency(&self) -> f64 {
            self.latencies.iter().copied().fold(0.0, f64::max)
        }

        /// Fracción del tiempo de sesión que el bucle pasó bloqueado en
        /// escrituras de vídeo. Es EL mecanismo del defecto: si el bucle se
        /// bloquea con el vídeo, el control espera.
        fn blocked_fraction(&self) -> f64 {
            if self.total_ms <= 0.0 {
                0.0
            } else {
                self.blocked_ms / self.total_ms
            }
        }
    }

    /// Corre el bucle de sesión (vídeo + control) sobre el canal modelado.
    ///
    /// El bucle es **secuencial** (como producción: `tokio::select!` con una
    /// sola tarea): si una escritura de vídeo bloquea, el control que llegue
    /// mientras tanto espera. El control NUNCA se descarta ni lo frena la
    /// compuerta; el vídeo sí se descarta.
    fn run(policy: Policy) -> Stats {
        let mut sock = Socket::default();
        let mut gate = VideoGate::new();
        let mut meter = DeliveryMeter::default();
        let mut st = Stats::default();
        let mut adapt = AdaptState::with_fps((1920, 1080), Some(15_000_000), 15);
        let mut t = 0.0f64;
        let mut next_frame = 0.0f64;
        let mut next_control = 0.0f64;
        let mut next_rtt = 0.0f64;
        // Se ejecuta hasta agotar TODOS los controles de la ventana.
        while next_control < RUN_MS {
            if next_frame <= next_control {
                t = t.max(next_frame);
                st.frames_attempted += 1;
                // Tamaño del frame: en B2 lo fija el presupuesto (CBR al
                // objetivo); en la Fase B, el piso de bpp a ciegas = el frame
                // de campo.
                let frame_bytes = match policy {
                    Policy::B2 => adapt.bps as f64 / 8.0 / 15.0,
                    Policy::FaseB => FIELD_FRAME_BYTES,
                };
                let cap = IN_FLIGHT_FRAMES * frame_bytes;
                if policy == Policy::FaseB || gate.allows(t) {
                    let (block_ms, _delivery) = sock.write(t, frame_bytes, cap);
                    gate.on_written(t, block_ms);
                    st.frames_written += 1;
                    st.blocked_ms += block_ms;
                    t += block_ms;
                    // Solo una escritura BLOQUEANTE mide el enlace.
                    if let Some(capacity) = meter.observe(frame_bytes as usize, block_ms) {
                        if let Some(dec) = adapt.observe_delivery(capacity) {
                            if dec.video_size.is_some() {
                                st.scale_changes += 1;
                            }
                        }
                    }
                } else {
                    gate.on_dropped();
                    st.frames_dropped += 1;
                }
                next_frame += FRAME_MS;
            } else {
                t = t.max(next_control);
                // El control va por el MISMO socket, pero la compuerta no lo
                // mira: se escribe siempre.
                let frame_bytes = match policy {
                    Policy::B2 => adapt.bps as f64 / 8.0 / 15.0,
                    Policy::FaseB => FIELD_FRAME_BYTES,
                };
                let cap = IN_FLIGHT_FRAMES * frame_bytes;
                let arrival = next_control;
                let (block_ms, delivery_ms) = sock.write(t, CONTROL_BYTES, cap);
                st.controls += 1;
                st.blocked_ms += block_ms;
                // Se mide TODA la ventana, incluido el arranque: el criterio del
                // gate es absoluto (el control nunca espera detrás del vídeo).
                st.latencies.push(delivery_ms - arrival);
                t += block_ms;
                next_control += CONTROL_MS;
            }
            // El controlador publica el RTT 1/s (sano: lo que manda es el
            // presupuesto; la congestión la detecta la entrega).
            if t >= next_rtt {
                adapt.observe_net(0);
                next_rtt += 1_000.0;
            }
        }
        st.total_ms = t;
        st
    }

    /// **Gate P1** `control_frames_are_not_starved_by_video`: sobre un enlace
    /// de **1 Mbps** real y con el frame de campo (131,5 KB), el control no
    /// puede esperar detrás del vídeo.
    ///
    /// - Con la política de B2 (compuerta + presupuesto): el vídeo que no cabe
    ///   se **descarta** (`frames_dropped > 0` — el contador
    ///   `video_dropped_slow` de producción) y la latencia del control queda
    ///   **por debajo de 1 s**.
    /// - En la Fase B (sin compuerta y con el encoder pidiendo el piso de bpp a
    ///   ciegas) la cola se queda llena y la latencia **crece** por encima de
    ///   1 s de forma sostenida.
    ///
    /// Comprueba el **mecanismo**, no solo el número: si alguien vuelve a
    /// bloquear el bucle de control con el vídeo (o deja de descartar), el
    /// tiempo bloqueado por el vídeo se dispara y el gate falla.
    #[test]
    fn control_frames_are_not_starved_by_video() {
        let b2 = run(Policy::B2);
        let fase_b = run(Policy::FaseB);

        // 1) La compuerta DESCARTA el vídeo que no cabe (frames frescos, no
        //    megabytes encolados delante de la entrada).
        assert!(
            b2.frames_dropped > 0,
            "la compuerta tiene que descartar: {b2:?}"
        );
        assert!(
            b2.frames_written < b2.frames_attempted,
            "se descarta vídeo: {}/{}",
            b2.frames_written,
            b2.frames_attempted
        );
        assert_eq!(
            fase_b.frames_dropped, 0,
            "la Fase B no descartaba nada (ese era el defecto)"
        );

        // 2) Criterio explícito: el control no espera detrás del vídeo. Con B2
        //    por debajo de 1 s; en la Fase B crece por encima de 1 s.
        assert!(
            b2.mean_latency() < 1_000.0,
            "latencia media del control con B2: {:.0} ms (criterio < 1000)",
            b2.mean_latency()
        );
        assert!(
            b2.max_latency() < 1_000.0,
            "ni el peor caso del control con B2: {:.0} ms (criterio < 1000)",
            b2.max_latency()
        );
        assert!(
            fase_b.mean_latency() >= 1_000.0,
            "sin compuerta el control espera detrás del vídeo: {:.0} ms",
            fase_b.mean_latency()
        );
        assert!(
            b2.mean_latency() * 4.0 < fase_b.mean_latency(),
            "la compuerta mejora la latencia del control (4×): {:.0} vs {:.0} ms",
            b2.mean_latency(),
            fase_b.mean_latency()
        );

        // 3) MECANISMO: el bucle no se bloquea con el vídeo. Con B2 la mayor
        //    parte del tiempo está libre para atender el control; en la Fase B
        //    el bucle vive dentro de `ch.send(video)`.
        assert!(
            b2.blocked_fraction() < 0.5,
            "el bucle pasa {:.0} % del tiempo bloqueado escribiendo vídeo (B2)",
            b2.blocked_fraction() * 100.0
        );
        assert!(
            fase_b.blocked_fraction() > 0.9,
            "la Fase B bloqueaba el bucle el {:.0} % del tiempo",
            fase_b.blocked_fraction() * 100.0
        );

        // 4) Las tramas de control NO se descartan nunca.
        assert_eq!(
            b2.controls,
            (RUN_MS / CONTROL_MS) as u32,
            "todos los controles de la ventana se entregaron"
        );
        assert_eq!(
            fase_b.controls,
            (RUN_MS / CONTROL_MS) as u32,
            "y también en la Fase B: el control nunca se descarta"
        );

        // 5) Y el presupuesto no oscila mientras se protege el control.
        assert!(
            b2.scale_changes <= 2,
            "la escalera no oscila durante el gate: {} cambios",
            b2.scale_changes
        );
    }

    /// El lazo COMPLETO de la Fase B2 sobre el enlace del gate de campo
    /// (~1 Mbps, 15 fps, 1080p): cuando el enlace es el cuello, la escritura
    /// tarda lo que tarda el enlace, y esa medida fija el objetivo. La escalera
    /// converge al presupuesto sin hundirse — es el test que habría cazado un
    /// techo auto-referencial: si la medida saliera de lo que OFRECEMOS, el
    /// objetivo caería 0,8× en cada ventana hasta cero.
    #[test]
    fn field_session_converges_to_the_delivered_budget() {
        const FPS: u32 = 15;
        let mut meter = DeliveryMeter::default();
        let mut st = AdaptState::with_fps((1920, 1080), Some(15_000_000), FPS);
        let mut changes = 0;
        let mut over_link_frames = 0;
        for i in 0..(FPS as usize * 20) {
            // El encoder produce lo que le pedimos (CBR al objetivo).
            let frame_bytes = st.bps as f64 / 8.0 / FPS as f64;
            // Escritura: por encima de lo que el enlace entrega, el socket no
            // puede aceptar más rápido que el enlace (es el cuello).
            let overflow = st.bps as f64 > LINK_BPS;
            let wait_ms = if overflow {
                over_link_frames += 1;
                frame_bytes / LINK_BYTES_PER_MS
            } else {
                0.0
            };
            if let Some(cap) = meter.observe(frame_bytes as usize, wait_ms) {
                if let Some(d) = st.observe_delivery(cap) {
                    if d.video_size.is_some() {
                        changes += 1;
                    }
                }
            }
            if i % FPS as usize == 0 {
                st.observe_net(0); // el controlador publica el RTT sano 1/s
                                   // Margen de 2 s para la primera medida (arranque conservador de
                                   // 2 Mbps sobre un enlace de 1 Mbps = 1 frame de más).
                if i >= FPS as usize * 2 {
                    assert!(
                        st.bps <= (LINK_BPS as usize).max(MIN_TARGET_BPS),
                        "el enlace entrega 1 Mbps y el objetivo pide {} bps",
                        st.bps
                    );
                }
            }
        }
        let cap = meter.bps().expect("el enlace de 1 Mbps se midió");
        assert!(
            (900_000..=1_100_000).contains(&cap),
            "capacidad medida ≈1 Mbps: {cap}"
        );
        let budget = (cap as f64 * DELIVERY_MARGIN) as usize;
        assert_eq!(st.bps, budget, "el objetivo ES el presupuesto");
        assert_eq!(st.size, (1000, 562), "se queda en el suelo de la escalera");
        assert!(changes <= 2, "sin oscilación: {changes} cambios de escala");
        assert!(
            over_link_frames < FPS as usize * 2,
            "deja de ofrecer de más en ~2 s: {over_link_frames} frames por encima del enlace"
        );
    }

    /// La compuerta solo se cierra con escrituras que se pasan del presupuesto
    /// y reabre sola: no es un interruptor permanente del vídeo.
    #[test]
    fn video_gate_closes_on_slow_writes_and_reopens() {
        let mut gate = VideoGate::new();
        assert!(gate.allows(0.0), "abierta con el enlace libre");
        // Escritura rápida: no cierra.
        gate.on_written(0.0, VIDEO_WRITE_BUDGET_MS * 0.5);
        assert!(
            gate.allows(0.0),
            "una escritura rápida no cierra la compuerta"
        );
        // Escritura lenta: cierra durante el hold.
        gate.on_written(100.0, VIDEO_WRITE_BUDGET_MS);
        assert!(!gate.allows(100.0 + VIDEO_DROP_HOLD_MS / 2.0), "cerrada");
        gate.on_dropped();
        gate.on_dropped();
        assert_eq!(gate.close_episode(100.0 + VIDEO_DROP_HOLD_MS / 2.0), None);
        assert_eq!(
            gate.close_episode(100.0 + VIDEO_DROP_HOLD_MS),
            Some(2),
            "el episodio se informa una sola vez"
        );
        assert!(gate.allows(100.0 + VIDEO_DROP_HOLD_MS), "y se reabre");
    }

    /// La medida de entrega solo se fía de una escritura BLOQUEANTE: es la
    /// única que mide el drenaje del enlace. Las instantáneas no publican nada
    /// (usarlas como techo hundiría el objetivo un 20 % por ventana: 0,8 × lo
    /// que ofrecemos no es la capacidad del enlace).
    #[test]
    fn delivery_meter_reads_the_link_and_not_the_target() {
        let mut m = DeliveryMeter::default();
        // Escrituras instantáneas: el socket las absorbe → no dicen nada.
        for _ in 0..50 {
            assert_eq!(m.observe(6_000, 0.0), None, "una escritura rápida no mide");
        }
        assert_eq!(m.bps(), None, "sin saturación la capacidad es desconocida");
        // 131,5 KB que tardan 1052 ms en escribirse ≈ 1 Mbps reales.
        let bps = m
            .observe(131_500, 1_052.0)
            .expect("una escritura bloqueante publica ya");
        assert!(
            (900_000..=1_100_000).contains(&bps),
            "≈1 Mbps entregados, no el objetivo: {bps}"
        );
        // Una variación pequeña NO se publica (no inunda de eventos/decisiones).
        assert_eq!(
            m.observe(131_500, 1_100.0),
            None,
            "una caída del 5 % no cambia la política"
        );
        // Otra escritura bloqueante más rápida NO sube la capacidad (subir es
        // lo que satura y hace oscilar la escalera).
        assert_eq!(
            m.observe(131_500, 200.0),
            None,
            "la capacidad no se reinfla"
        );
        assert_eq!(m.bps(), Some(bps), "sigue la medida original");
        // Una caída material sí la baja: es evidencia de un enlace peor.
        let low = m.observe(131_500, 2_000.0).expect("publica");
        assert!(low < bps, "el enlace empeoró: {low} < {bps}");
    }
}
