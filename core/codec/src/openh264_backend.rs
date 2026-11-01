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
}

impl H264Decoder {
    pub fn new() -> Result<Self, CodecError> {
        let inner =
            Decoder::new().map_err(|e| CodecError::Decode(format!("openh264 new: {e:?}")))?;
        Ok(Self { inner })
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
        let yuv = self
            .inner
            .decode(&frame.data)
            .map_err(|e| CodecError::Decode(format!("openh264: {e:?}")))?;
        // `None` = el decoder aún no tiene frame completo (p. ej. solo
        // SPS/PPS): mismo significado que NEED_MORE_INPUT en el backend MF.
        let yuv =
            yuv.ok_or_else(|| CodecError::Decode("bitstream incompleto (NEED_MORE_INPUT)".into()))?;
        let (w, h) = yuv.dimensions();
        if w == 0 || h == 0 {
            return Err(CodecError::Decode("dims decodificadas nulas".into()));
        }
        // RGB8 (la conversión de color la hace el propio openh264) → BGRA8,
        // el formato canónico del pipeline (H-RD-05).
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CodecFormat, VideoEncoder};
    use anyremote_capture::TestPatternCapturer;

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
