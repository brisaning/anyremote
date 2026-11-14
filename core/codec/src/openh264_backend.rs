//! Decodificación H.264 con **Cisco OpenH264** (licencia BSD) — C-14.
//!
//! Decisión (usuario autorizada a usar terceros): el decoder MFT de Microsoft
//! aceptaba toda la entrada y **nunca emitía salida** pese a una configuración
//! conforme (matriz exhaustiva documentada en `evidence/hwcodec-gate.md`), y
//! el stream resultó **válido** con ffmpeg/libav → se usa el decodificador de
//! referencia BSD en lugar de perseguir el MFT opaco (siguiente alternativa
//! habría sido D3D11/DXVA, esfuerzo L). El ENCODER sigue siendo MF (funciona
//! y aprovecha la GPU cuando hay un MFT sync disponible; si no, VP9/libvpx).

use anyremote_capture::{Frame, PixelFormat};
use openh264::decoder::Decoder;
use openh264::formats::YUVSource; // trait con dimensions()

use crate::{CodecError, EncodedFrame, VideoDecoder};

/// Decodificador H.264 con estado (openh264 auto-gestiona SPS/PPS in-band y
/// el retardo de reordenación — `Flush::Auto` por defecto).
pub struct H264Decoder {
    inner: Decoder,
    /// C-26: resets auto-sanadores realizados (se loguean los primeros3).
    resets: u8,
    /// C-27: esperando un IDR tras auto-reset (se saltan los frames sin
    /// referencia en vez de acumular `native:18`).
    waiting_idr: bool,
}

impl H264Decoder {
    pub fn new() -> Result<Self, CodecError> {
        let inner =
            Decoder::new().map_err(|e| CodecError::Decode(format!("openh264 new: {e:?}")))?;
        Ok(Self {
            inner,
            resets: 0,
            waiting_idr: false,
        })
    }
}

impl Default for H264Decoder {
    fn default() -> Self {
        Self::new().expect("openh264 decoder")
    }
}

impl VideoDecoder for H264Decoder {
    fn decode(&mut self, frame: &EncodedFrame) -> Result<Frame, CodecError> {
        if frame.data.is_empty() {
            return Err(CodecError::Decode("frame H.264 vacío".into()));
        }
        // C-27: tras un auto-reset el decoder está vacío — mientras no
        // llegue un IDR no hay referencia que valga: se SALTAN los frames
        // (sin `native:18` en el registro; en campo eran cientos por
        // sesión) y el IDR reconstruye el estado.
        if self.waiting_idr && !has_idr_nal(&frame.data) {
            return Err(CodecError::Skipped);
        }
        // C-26/C-27: errores de parámetros (0x10 params /0x20 no-params) →
        // reset + esperar IDR (reintento inmediato si el frame ES un IDR).
        // Primera pasada con salida OWNED: el borrow del decode queda
        // consumido antes de poder reasignar `self.inner`.
        let params_code: Option<i64> = match self.inner.decode(&frame.data) {
            Ok(Some(y)) => {
                self.waiting_idr = false;
                return yuv_to_frame(y);
            }
            Ok(None) => None,
            Err(e) if e.native_code() & 0x30 != 0 => Some(e.native_code()),
            Err(e) => return Err(CodecError::Decode(format!("openh264: {e:?}"))),
        };
        if let Some(code) = params_code {
            if self.resets < 3 {
                self.resets += 1;
                eprintln!("[video] decoder H.264 auto-reset (params native={code})");
            }
            self.inner = Decoder::new()
                .map_err(|er| CodecError::Decode(format!("openh264 reset: {er:?}")))?;
            self.waiting_idr = true;
            if !has_idr_nal(&frame.data) {
                return Err(CodecError::Skipped); // P: esperar al próximo IDR
            }
            // El frame ES un IDR con los parámetros nuevos → reintento ya.
            self.waiting_idr = false;
            return match self.inner.decode(&frame.data) {
                Ok(Some(y)) => yuv_to_frame(y),
                Ok(None) | Err(_) => Err(CodecError::Skipped),
            };
        }
        // `None` = el decoder aún no tiene frame completo (p. ej. solo
        // SPS/PPS): mismo significado que NEED_MORE_INPUT en el backend MF.
        Err(CodecError::Decode(
            "bitstream incompleto (NEED_MORE_INPUT)".into(),
        ))
    }
}

/// Convierte un frame YUV decodificado a BGRA8 (formato canónico H-RD-05).
fn yuv_to_frame(yuv: openh264::decoder::DecodedYUV<'_>) -> Result<Frame, CodecError> {
    let (w, h) = yuv.dimensions();
    if w == 0 || h == 0 {
        return Err(CodecError::Decode("dims decodificadas nulas".into()));
    }
    // RGB8 (la conversión de color la hace el propio openh264) → BGRA8.
    let mut rgb = vec![0u8; w * h * 3];
    yuv.write_rgb8(&mut rgb);
    let mut bgra = Vec::with_capacity(w * h * 4);
    for p in rgb.as_chunks::<3>().0 {
        bgra.extend_from_slice(&[p[2], p[1], p[0], 255]);
    }
    Ok(Frame {
        width: w as u32,
        height: h as u32,
        format: PixelFormat::Bgra8,
        data: bgra,
    })
}

/// C-27: ¿el payload contiene un IDR (NAL tipo5)? Tras un auto-reset el
/// decoder solo puede reconstruirse desde un IDR.
fn has_idr_nal(b: &[u8]) -> bool {
    let mut i = 0;
    while let Some(p) = crate::mf_backend::find_start_code(b, i) {
        let long = b[p..].starts_with(&[0, 0, 0, 1]);
        let nal = p + if long { 4 } else { 3 };
        if let Some(first) = b.get(nal) {
            if first & 0x1F == 5 {
                return true;
            }
        }
        i = nal;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CodecFormat, VideoEncoder};
    use anyremote_capture::TestPatternCapturer;

    /// C-27: detección de IDR (NAL tipo5) para el salto tras auto-reset.
    #[test]
    fn has_idr_nal_detects_idr_slices() {
        assert!(has_idr_nal(&[0, 0, 0, 1, 0x65, 0x88]), "IDR");
        assert!(!has_idr_nal(&[0, 0, 0, 1, 0x41, 0x9A]), "P-slice");
        assert!(!has_idr_nal(&[0, 0, 0, 1, 0x67, 0x42]), "SPS no es IDR");
    }

    /// C-26: con un cambio de parámetros en caliente (dims distintas →
    /// SPS nuevo) el decoder se AUTO-SANifica en vez de quedarse
    /// envenenado (campo juank2: 243/261 frames perdidos con `native:18`).
    #[test]
    fn decoder_recovers_from_param_change() {
        use crate::mf_backend::MfH264Encoder;
        if crate::mf_backend::mf_ready().is_err() {
            eprintln!("MF no disponible: se omite");
            return;
        }
        let mut enc = MfH264Encoder::new(15, 600_000);
        let mut dec = H264Decoder::new().expect("openh264");
        // Calienta y decodifica a320x180.
        let f1 = TestPatternCapturer::render(320, 180, 7);
        let mut warm = false;
        for _ in 0..40 {
            let e = enc.encode(&f1).expect("encode");
            if e.data.is_empty() {
                continue;
            }
            if dec.decode(&e).is_ok() {
                warm = true;
                break;
            }
        }
        assert!(warm, "el warmup debe decodificar");
        // Cambio de parámetros en caliente:160x90 → SPS distinto.
        let f2 = TestPatternCapturer::render(160, 90, 7);
        let mut recovered = false;
        let mut skipped = 0u32;
        for _ in 0..60 {
            let e = enc.encode(&f2).expect("encode dims nuevas");
            if e.data.is_empty() {
                continue;
            }
            match dec.decode(&e) {
                Ok(_) => {
                    recovered = true;
                    break;
                }
                // C-27: esperando IDR = silencioso y permitido.
                Err(CodecError::Skipped) => skipped += 1,
                // C-27: NINGÚN error de decode (p. ej. native:18) debe
                // escapar tras el auto-reset — el registro se inundaba.
                Err(other) => panic!("no debe escapar error de decode: {other:?}"),
            }
        }
        assert!(
            recovered,
            "el decoder debe auto-sanarse con el SPS nuevo (reset+IDR)"
        );
        eprintln!("recuperado con {skipped} frames saltados (esperado: ≥0)");
    }

    /// Roundtrip real: encoder MF de Microsoft → decoder OpenH264 (B→C).
    #[test]
    fn h264_roundtrip_mf_encoder_openh264_decoder() {
        use crate::mf_backend::MfH264Encoder;
        if crate::mf_backend::mf_ready().is_err() {
            eprintln!("MF no disponible: se omite");
            return;
        }
        let frame = TestPatternCapturer::render(320, 180, 7);
        let mut enc = MfH264Encoder::new(15, 600_000);
        let mut dec = H264Decoder::new().expect("openh264");
        let mut out: Option<Frame> = None;
        let mut nals = 0usize;
        for _ in 0..40 {
            let f = enc.encode(&frame).expect("encode H.264");
            if f.data.is_empty() {
                continue;
            }
            nals += 1;
            assert_eq!(f.format, CodecFormat::H264);
            assert_eq!((f.width, f.height), (320, 180), "dims pares preservados");
            assert!(
                f.data.windows(4).any(|w| w == [0, 0, 0, 1]),
                "Annex B con start code"
            );
            match dec.decode(&f) {
                Ok(fr) => {
                    out = Some(fr);
                    break;
                }
                Err(CodecError::Decode(e)) if e.contains("NEED_MORE_INPUT") => continue,
                Err(e) => panic!("decode H.264: {e:?}"),
            }
        }
        let out = out.unwrap_or_else(|| {
            panic!("el decoder no produjo frame con {nals} NALs (stream válido según ffmpeg)")
        });
        assert_eq!((out.width, out.height), (320, 180));
        assert_eq!(out.format, PixelFormat::Bgra8);
        assert_eq!(out.data.len(), 320 * 180 * 4);
        assert!(out.data.iter().any(|&b| b > 0), "no todo negro");
    }
}
