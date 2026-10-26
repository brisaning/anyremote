//! Backend VP9 real (ciclo C3-bis, D-06) sobre `shiguredo_libvpx` (binario precompilado,
//! Apache-2.0; bindings con símbolos prefijados). Conversión BGRA8 ↔ I420 propia
//! (VP9 trabaja en YUV 4:2:0). Clean-room D-01.

use std::collections::VecDeque;

use shiguredo_libvpx::{
    CodecConfig, Decoder, DecoderCodec, DecoderConfig, EncodeOptions, Encoder, EncoderConfig,
    EncodingDeadline, ImageData, ImageFormat, RateControlMode, Vp9Config,
};

use anyremote_capture::{Frame, PixelFormat};

use crate::{CodecError, CodecFormat, EncodedFrame, VideoDecoder, VideoEncoder};

/// Keyframe periódico (resincronización; patrón GOP mínimo C3-bis).
const KEYFRAME_INTERVAL: u32 = 60;

/// BGRA8 → I420 (BT.601). Croma promediado 2x2.
pub fn bgra_to_i420(width: u32, height: u32, bgra: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let w = width as usize;
    let h = height as usize;
    debug_assert!(w % 2 == 0 && h % 2 == 0, "dimensiones pares requeridas (4:2:0)");
    let mut y = vec![0u8; w * h];
    let cw = w / 2;
    let mut u = vec![0u8; cw * (h / 2)];
    let mut v = vec![0u8; cw * (h / 2)];

    for row in 0..h {
        for col in 0..w {
            let i = (row * w + col) * 4;
            let b = bgra[i] as i32;
            let g = bgra[i + 1] as i32;
            let r = bgra[i + 2] as i32;
            let yy = ((77 * r + 150 * g + 29 * b) >> 8); // rango completo sin offset
            y[row * w + col] = yy.clamp(0, 255) as u8;
            if row % 2 == 0 && col % 2 == 0 {
                let mut sb = 0i32;
                let mut sg = 0i32;
                let mut sr = 0i32;
                for dy in 0..2usize {
                    for dx in 0..2usize {
                        let j = ((row + dy) * w + (col + dx)) * 4;
                        sb += bgra[j] as i32;
                        sg += bgra[j + 1] as i32;
                        sr += bgra[j + 2] as i32;
                    }
                }
                let (b, g, r) = (sb / 4, sg / 4, sr / 4);
                let uu = ((-43 * r - 85 * g + 128 * b + 128) >> 8) + 128;
                let vv = ((128 * r - 107 * g - 21 * b + 128) >> 8) + 128;
                let ci = (row / 2) * cw + col / 2;
                u[ci] = uu.clamp(0, 255) as u8;
                v[ci] = vv.clamp(0, 255) as u8;
            }
        }
    }
    (y, u, v)
}

/// I420 → BGRA8 (BT.601; croma por muestreo simple).
pub fn i420_to_bgra(width: usize, height: usize, y: &[u8], u: &[u8], v: &[u8], y_s: usize, c_s: usize) -> Vec<u8> {
    let mut out = vec![0u8; width * height * 4];
    for row in 0..height {
        for col in 0..width {
            let yy = y[row * y_s + col] as i32; // rango completo (par coherente con bgra_to_i420)
            let uu = u[(row / 2) * c_s + col / 2] as i32;
            let vv = v[(row / 2) * c_s + col / 2] as i32;
            let ep = vv - 128;
            let eu = uu - 128;
            let r = (256 * yy + 359 * ep + 128) >> 8;
            let g = (256 * yy - 88 * eu - 183 * ep + 128) >> 8;
            let b = (256 * yy + 453 * eu + 128) >> 8;
            let i = (row * width + col) * 4;
            out[i] = b.clamp(0, 255) as u8;
            out[i + 1] = g.clamp(0, 255) as u8;
            out[i + 2] = r.clamp(0, 255) as u8;
            out[i + 3] = 255;
        }
    }
    out
}

/// Encoder VP9 con estado GOP (keyframe inicial + periódico) y cola de paquetes.
pub struct Vp9Encoder {
    inner: Option<Encoder>,
    seq: u64,
    since_key: u32,
    pending: VecDeque<Vec<u8>>,
    width: u32,
    height: u32,
}

impl Vp9Encoder {
    pub fn new() -> Self {
        Self {
            inner: None,
            seq: 0,
            since_key: 0,
            pending: VecDeque::new(),
            width: 0,
            height: 0,
        }
    }

    fn ensure(&mut self, width: u32, height: u32) -> Result<(), CodecError> {
        if self.inner.is_none() {
            let mut cfg = EncoderConfig::new(
                width as usize,
                height as usize,
                ImageFormat::I420,
                CodecConfig::Vp9(Vp9Config::default()),
            );
            // Bitrate proporcional al área (~3 bits/px/frame → comprime ~10x a 30fps).
            cfg.target_bitrate = (width as usize * height as usize * 3).max(150_000);
            cfg.deadline = EncodingDeadline::Realtime;
            cfg.rate_control = RateControlMode::Cbr;
            cfg.min_quantizer = 4;
            cfg.max_quantizer = 52;
            cfg.cpu_used = Some(6);
            cfg.lag_in_frames = std::num::NonZeroUsize::new(1); // wrapper no admite 0; lag=1 = 1 frame de retardo (~33 ms)
            cfg.error_resilient = true;
            cfg.frame_drop_threshold = None; // no omitir frames en el pipeline de sesión
            cfg.keyframe_interval = std::num::NonZeroUsize::new(KEYFRAME_INTERVAL as usize);
            self.width = width;
            self.height = height;
            self.inner = Some(
                Encoder::new(cfg).map_err(|e| CodecError::Encode(format!("encoder VP9: {e:?}")))?,
            );
        }
        Ok(())
    }
}

impl Default for Vp9Encoder {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoEncoder for Vp9Encoder {
    fn encode(&mut self, frame: &Frame) -> Result<EncodedFrame, CodecError> {
        self.ensure(frame.width, frame.height)?;
        let inner = self.inner.as_mut().expect("inicializado");

        if self.pending.is_empty() {
            let (y, u, v) = bgra_to_i420(frame.width, frame.height, &frame.data);
            let force = self.since_key == 0 || self.since_key >= KEYFRAME_INTERVAL;
            inner
                .encode(
                    &ImageData::I420 { y: &y, u: &u, v: &v },
                    &EncodeOptions { force_keyframe: force },
                )
                .map_err(|e| CodecError::Encode(format!("encode: {e:?}")))?;
            self.since_key += 1;
            while let Some(pkt) = inner.next_frame() {
                self.pending.push_back(pkt.data().to_vec());
            }
        }

        let seq = self.seq;
        self.seq += 1;
        let data = self.pending.pop_front().unwrap_or_default();
        Ok(EncodedFrame {
            format: CodecFormat::Vp9,
            seq,
            width: frame.width,
            height: frame.height,
            data, // vacío = frame omitido por el rate control (documentado)
        })
    }

    fn format(&self) -> CodecFormat {
        CodecFormat::Vp9
    }
}

/// Decoder VP9 con estado de sesión.
pub struct Vp9Decoder {
    inner: Decoder,
    width: u32,
    height: u32,
}

impl Vp9Decoder {
    pub fn new() -> Result<Self, CodecError> {
        let inner = Decoder::new(DecoderConfig {
            codec: DecoderCodec::Vp9,
        })
        .map_err(|e| CodecError::Decode(format!("decoder VP9: {e:?}")))?;
        Ok(Self {
            inner,
            width: 0,
            height: 0,
        })
    }
}

impl Default for Vp9Decoder {
    fn default() -> Self {
        Self::new().expect("decoder VP9")
    }
}

impl VideoDecoder for Vp9Decoder {
    fn decode(&mut self, frame: &EncodedFrame) -> Result<Frame, CodecError> {
        if frame.data.is_empty() {
            return Err(CodecError::Decode("frame omitida (vacía)".into()));
        }
        self.inner
            .decode(&frame.data)
            .map_err(|e| CodecError::Decode(format!("decode: {e:?}")))?;
        let img = self
            .inner
            .next_frame()
            .map_err(|e| CodecError::Decode(format!("next_frame: {e:?}")))?
            .ok_or_else(|| CodecError::Decode("sin frame decodificado".into()))?;

        let (w, h) = (img.width(), img.height());
        if w != frame.width as usize || h != frame.height as usize {
            return Err(CodecError::Decode(format!(
                "dimensión decodificada {w}x{h} != esperada {}x{}",
                frame.width, frame.height
            )));
        }
        let data = i420_to_bgra(w, h, img.y_plane(), img.u_plane(), img.v_plane(), img.y_stride(), img.u_stride());
        self.width = w as u32;
        self.height = h as u32;
        Ok(Frame {
            width: self.width,
            height: self.height,
            format: PixelFormat::Bgra8,
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyremote_capture::TestPatternCapturer;

    #[test]
    fn i420_roundtrip_of_flat_color_is_accurate() {
        // Color plano → la conversión debe ser casi exacta (sin submuestreo en plano).
        let w = 64u32;
        let h = 64u32;
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        for px in bgra.chunks_exact_mut(4) {
            px[0] = 40;
            px[1] = 160;
            px[2] = 220;
            px[3] = 255;
        }
        let (y, u, v) = bgra_to_i420(w, h, &bgra);
        let back = i420_to_bgra(w as usize, h as usize, &y, &u, &v, w as usize, w as usize / 2);
        let px = &back[0..3];
        // Tolerancia razonable (redondeo BT.601 ida/vuelta).
        assert!(px[0].abs_diff(40) <= 3, "B {}", px[0]);
        assert!(px[1].abs_diff(160) <= 3, "G {}", px[1]);
        assert!(px[2].abs_diff(220) <= 3, "R {}", px[2]);
    }

    #[test]
    fn vp9_encode_decode_lockstep_preserves_geometry() {
        let mut enc = Vp9Encoder::new();
        let mut dec = Vp9Decoder::new().expect("decoder");
        let (w, h) = (176u32, 144u32); // múltiplos de 16 (segmentación VP9)

        let mut encoded = 0usize;
        let mut decoded = 0usize;
        for n in 0..90u64 {
            let frame = TestPatternCapturer::render(w, h, n);
            let out = enc.encode(&frame).expect("encode");
            if out.data.is_empty() {
                continue;
            }
            encoded += 1;
            assert_eq!(out.format, CodecFormat::Vp9);
            let back = dec.decode(&out).expect("decode");
            assert_eq!((back.width, back.height), (w, h));
            assert_eq!(back.data.len(), (w * h * 4) as usize);
            decoded += 1;
        }
        assert!(encoded >= 85, "solo {encoded} frames codificados");
        assert_eq!(decoded, encoded);
    }
}
