//! `anyremote-codec` — Traits de códec de vídeo y backend inicial (D-06, patrón H-RD-05:
//! fábrica `Encoder`/`Decoder` con `CodecFormat` negociable). Backend inicial: Raw y
//! Deflate (lossless, sin dependencias C) — el backend VP8/VP9 real se añade tras
//! habilitar libvpx en el host (gap documentado en C3). Clean-room D-01.

use anyremote_capture::{CaptureError, Frame, PixelFormat};

/// Formato negociable del códec (patrón H-RD-05 `CodecFormat`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CodecFormat {
    Raw = 0,
    Deflate = 1,
    Vp9 = 2,
}

impl CodecFormat {
    pub fn from_proto(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Raw),
            1 => Some(Self::Deflate),
            2 => Some(Self::Vp9),
            _ => None,
        }
    }
}

/// Frame empaquetado listo para transportar.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EncodedFrame {
    pub format: CodecFormat,
    pub seq: u64,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub enum CodecError {
    Encode(String),
    Decode(String),
    Unsupported(CodecFormat),
    Capture(CaptureError),
}

pub trait VideoEncoder: Send {
    fn encode(&mut self, frame: &Frame) -> Result<EncodedFrame, CodecError>;
    fn format(&self) -> CodecFormat;
}

pub trait VideoDecoder: Send {
    fn decode(&mut self, frame: &EncodedFrame) -> Result<Frame, CodecError>;
}

/// Codificador passthrough (referencia y debugging).
pub struct RawEncoder {
    seq: u64,
}

impl RawEncoder {
    pub fn new() -> Self {
        Self { seq: 0 }
    }
}

impl Default for RawEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoEncoder for RawEncoder {
    fn encode(&mut self, frame: &Frame) -> Result<EncodedFrame, CodecError> {
        let out = EncodedFrame {
            format: CodecFormat::Raw,
            seq: self.seq,
            width: frame.width,
            height: frame.height,
            data: frame.data.clone(),
        };
        self.seq += 1;
        Ok(out)
    }

    fn format(&self) -> CodecFormat {
        CodecFormat::Raw
    }
}

/// Codificador lossless Deflate (zlib) — primer backend real del pipeline.
pub struct DeflateEncoder {
    seq: u64,
    level: u32,
}

impl DeflateEncoder {
    pub fn new(level: u32) -> Self {
        Self { seq: 0, level }
    }
}

impl VideoEncoder for DeflateEncoder {
    fn encode(&mut self, frame: &Frame) -> Result<EncodedFrame, CodecError> {
        use std::io::Write;
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(self.level));
        e.write_all(&frame.data)
            .map_err(|err| CodecError::Encode(err.to_string()))?;
        let data = e
            .finish()
            .map_err(|err| CodecError::Encode(err.to_string()))?;
        let out = EncodedFrame {
            format: CodecFormat::Deflate,
            seq: self.seq,
            width: frame.width,
            height: frame.height,
            data,
        };
        self.seq += 1;
        Ok(out)
    }

    fn format(&self) -> CodecFormat {
        CodecFormat::Deflate
    }
}

/// Decodificador según el formato del frame (fábrica implícita).
pub struct NativeDecoder;

impl NativeDecoder {
    pub fn new() -> Self {
        Self
    }
}

impl VideoDecoder for NativeDecoder {
    fn decode(&mut self, frame: &EncodedFrame) -> Result<Frame, CodecError> {
        use std::io::Read;
        let expect = (frame.width * frame.height * 4) as usize;
        let data = match frame.format {
            CodecFormat::Raw => frame.data.clone(),
            CodecFormat::Deflate => {
                let mut out = Vec::with_capacity(expect);
                flate2::read::ZlibDecoder::new(&frame.data[..])
                    .read_to_end(&mut out)
                    .map_err(|e| CodecError::Decode(e.to_string()))?;
                out
            }
            CodecFormat::Vp9 => {
                // Gateado por feature `vpx` (ciclo C3-bis): sin libvpx, se rechaza explícitamente.
                return Err(CodecError::Unsupported(CodecFormat::Vp9));
            }
        };
        if data.len() != expect {
            return Err(CodecError::Decode(format!(
                "tamaño decodificado {} inesperado (esperado {expect})",
                data.len()
            )));
        }
        Ok(Frame {
            width: frame.width,
            height: frame.height,
            format: PixelFormat::Bgra8,
            data,
        })
    }
}

/// Encoder que rechaza explícitamente un formato no soportado en este build.
pub struct UnsupportedEncoder {
    format: CodecFormat,
}

impl VideoEncoder for UnsupportedEncoder {
    fn encode(&mut self, _frame: &Frame) -> Result<EncodedFrame, CodecError> {
        Err(CodecError::Unsupported(self.format))
    }

    fn format(&self) -> CodecFormat {
        self.format
    }
}

/// Fábrica de encoders por formato.
pub fn encoder_for(format: CodecFormat) -> Box<dyn VideoEncoder> {
    match format {
        CodecFormat::Raw => Box::new(RawEncoder::new()),
        CodecFormat::Deflate => Box::new(DeflateEncoder::new(6)),
        // C3-bis: se reemplaza por Vp9Encoder con feature `vpx` (libvpx).
        CodecFormat::Vp9 => Box::new(UnsupportedEncoder {
            format: CodecFormat::Vp9,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyremote_capture::TestPatternCapturer;

    #[test]
    fn deflate_roundtrip_is_lossless_and_compresses() {
        let frame = TestPatternCapturer::render(320, 200, 3);
        let mut enc = DeflateEncoder::new(6);
        let mut dec = NativeDecoder::new();

        let enc_f = enc.encode(&frame).expect("encode");
        assert_eq!(enc_f.format, CodecFormat::Deflate);
        assert!(enc_f.data.len() < frame.data.len(), "debe comprimir");
        let dec_f = dec.decode(&enc_f).expect("decode");
        assert_eq!(dec_f, frame);
    }

    #[test]
    fn raw_roundtrip_is_lossless() {
        let frame = TestPatternCapturer::render(64, 64, 9);
        let mut enc = encoder_for(CodecFormat::Raw);
        let mut dec = NativeDecoder::new();
        let enc_f = enc.encode(&frame).unwrap();
        assert_eq!(dec.decode(&enc_f).unwrap(), frame);
    }

    #[test]
    fn decode_rejects_corrupt_size() {
        let mut dec = NativeDecoder::new();
        let bad = EncodedFrame {
            format: CodecFormat::Raw,
            seq: 0,
            width: 10,
            height: 10,
            data: vec![0u8; 5],
        };
        assert!(matches!(dec.decode(&bad), Err(CodecError::Decode(_))));
    }

    #[test]
    fn codec_format_maps_from_proto() {
        assert_eq!(CodecFormat::from_proto(1), Some(CodecFormat::Deflate));
        assert_eq!(CodecFormat::from_proto(99), None);
    }
}
