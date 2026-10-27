//! Canal ARSP: mensajes de sesión (`arsp::Envelope`) sobre los frames del transporte.
//! Ciclo C1 (RF-01). Patrones: H-RD-01/H-RD-03. Clean-room D-01.

use prost::Message;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{write_frame, TransportError};

pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/arsp.rs"));
}

/// Versión del protocolo ARSP soportada por este build (D-05).
pub const ARSP_VERSION: u32 = 1;

/// Canal ARSP genérico sobre cualquier stream (TLS en producción, TCP en tests).
///
/// **Cancellation-safe (fix v0.1.6)**: `recv()` acumula bytes en este buffer
/// PERSISTENTE. Si un `tokio::select!` cancela la recepción a mitad (ticks de
/// latido/portapapeles), los bytes ya leídos permanecen en `buf` — antes se
/// perdían y desincronizaban el flujo (bytes de payload leídos como longitud →
/// `FrameTooLarge` y muerte de sesión en pruebas cross-device reales).
pub struct Channel<T> {
    inner: T,
    next_seq: u64,
    buf: Vec<u8>,
}

impl<T> Channel<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            next_seq: 1,
            buf: Vec::with_capacity(16 * 1024),
        }
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> Channel<T> {
    /// Codifica y envía un payload como Envelope ARSP con seq correlativo.
    pub async fn send(&mut self, payload: proto::envelope::Payload) -> Result<(), TransportError> {
        let env = proto::Envelope {
            seq: self.next_seq,
            payload: Some(payload),
        };
        self.next_seq += 1;
        let mut buf = Vec::with_capacity(env.encoded_len());
        env.encode(&mut buf).map_err(|e| TransportError::Io(e.to_string()))?;
        write_frame(&mut self.inner, &buf).await
    }

    /// Recibe y decodifica un Envelope ARSP. **Cancel-safe**: el progreso parcial
    /// vive en el buffer del canal, no en el futuro cancelado.
    pub fn recv(
        &mut self,
    ) -> impl std::future::Future<Output = Result<proto::Envelope, TransportError>> + '_ {
        std::future::poll_fn(move |cx| {
            use std::task::Poll;
            use std::pin::Pin;
            loop {
                // ¿Frame completo ya bufferizado?
                if self.buf.len() >= 4 {
                    let len = u32::from_be_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]])
                        as usize;
                    if len > crate::MAX_FRAME_BYTES {
                        self.buf.clear();
                        return Poll::Ready(Err(TransportError::FrameTooLarge(len)));
                    }
                    if self.buf.len() >= 4 + len {
                        let frame = self.buf[4..4 + len].to_vec();
                        self.buf.drain(..4 + len);
                        return Poll::Ready(
                            proto::Envelope::decode(frame.as_slice())
                                .map_err(|e| TransportError::Io(e.to_string())),
                        );
                    }
                }
                // Más bytes al buffer persistente (si poll queda Pending no se consumió nada;
                // si consumió, el byte queda en self.buf y sobrevive a la cancelación).
                let mut chunk = [0u8; 16 * 1024];
                let mut rb = tokio::io::ReadBuf::new(&mut chunk);
                match Pin::new(&mut self.inner).poll_read(cx, &mut rb) {
                    Poll::Ready(Ok(())) => {
                        let n = rb.filled().len();
                        if n == 0 {
                            return Poll::Ready(Err(TransportError::Eof));
                        }
                        self.buf.extend_from_slice(rb.filled());
                    }
                    Poll::Ready(Err(e)) => {
                        return Poll::Ready(Err(TransportError::Io(e.to_string())))
                    }
                    Poll::Pending => return Poll::Pending,
                }
            }
        })
    }
}

/// Payload de conveniencia para Hello.
pub fn hello(node_id: &str, session_hint: &str) -> proto::envelope::Payload {
    proto::envelope::Payload::Hello(proto::Hello {
        proto_version: ARSP_VERSION,
        node_id: node_id.to_string(),
        session_hint: session_hint.to_string(),
    })
}

/// Payload de conveniencia para Welcome con todas las capacidades publicadas.
pub fn welcome(node_id: &str) -> proto::envelope::Payload {
    proto::envelope::Payload::Welcome(proto::Welcome {
        proto_version: ARSP_VERSION,
        node_id: node_id.to_string(),
        capabilities: Some(proto::SessionCapabilities {
            keyboard: true,
            mouse: true,
            clipboard: true,
            audio: true,
            files: true,
            supported_codecs: vec![
                proto::CodecFormatEnum::RawCodec as i32,
                proto::CodecFormatEnum::DeflateCodec as i32,
                proto::CodecFormatEnum::Vp9Codec as i32,
            ],
        }),
    })
}

/// Extrae el node_id anunciado en un Hello/Welcome, si corresponde.
pub fn announced_node_id(env: &proto::Envelope) -> Option<String> {
    match env.payload {
        Some(proto::envelope::Payload::Hello(ref h)) => Some(h.node_id.clone()),
        Some(proto::envelope::Payload::Welcome(ref w)) => Some(w.node_id.clone()),
        _ => None,
    }
}

/// Valida la versión de protocolo en Hello/Welcome.
pub fn check_version(env: &proto::Envelope) -> Result<(), String> {
    let v = match env.payload {
        Some(proto::envelope::Payload::Hello(ref h)) => h.proto_version,
        Some(proto::envelope::Payload::Welcome(ref w)) => w.proto_version,
        _ => return Ok(()),
    };
    if v == ARSP_VERSION {
        Ok(())
    } else {
        Err(format!("versión ARSP {v} no soportada (esperada {ARSP_VERSION})"))
    }
}

/// Convierte un frame empaquetado del códec a su forma wire ARSP.
pub fn video_frame_to_wire(frame: &anyremote_codec::EncodedFrame) -> proto::VideoFrame {
    proto::VideoFrame {
        seq: frame.seq,
        width: frame.width,
        height: frame.height,
        format: frame.format as i32,
        data: frame.data.clone(),
    }
}

/// Convierte un VideoFrame wire al frame empaquetado del códec (valida formato).
pub fn video_frame_from_wire(v: proto::VideoFrame) -> Result<anyremote_codec::EncodedFrame, TransportError> {
    let format = anyremote_codec::CodecFormat::from_proto(v.format)
        .ok_or_else(|| TransportError::Io(format!("formato de códec desconocido: {}", v.format)))?;
    Ok(anyremote_codec::EncodedFrame {
        format,
        seq: v.seq,
        width: v.width,
        height: v.height,
        data: v.data,
    })
}

/// Contenido de portapapeles → wire (solo formatos admitidos por contención C4).
pub fn clipboard_to_wire(
    content: &anyremote_clipboard::ClipboardContent,
) -> Result<proto::ClipboardSync, TransportError> {
    let format = match content.format {
        anyremote_clipboard::ClipboardFormat::Text => proto::ClipboardFormatEnum::ClipText,
    };
    Ok(proto::ClipboardSync {
        revision: content.revision,
        format: format as i32,
        utf8: content.text.as_bytes().to_vec(),
    })
}

/// Wire → contenido de portapapeles; **rechaza formatos no admitidos** (contención).
pub fn clipboard_from_wire(
    v: proto::ClipboardSync,
) -> Result<anyremote_clipboard::ClipboardContent, TransportError> {
    let format = anyremote_clipboard::ClipboardFormat::from_wire(v.format).ok_or_else(|| {
        TransportError::Auth(format!("formato de portapapeles no admitido: {}", v.format))
    })?;
    Ok(anyremote_clipboard::ClipboardContent {
        revision: v.revision,
        format,
        text: String::from_utf8_lossy(&v.utf8).into_owned(),
    })
}

/// Código de tecla wire → tecla del modelo de inyección (RF-02a).
/// Tabla compartida app↔daemon; el espejo documentado vive en `arsp.proto::Key`.
fn key_from_code(code: u32, ch: &str) -> Option<anyremote_input::Key> {
    use anyremote_input::Key;
    match code {
        // Carácter: un solo codepoint UTF-8 (letra, dígito, símbolo).
        0 => {
            let mut it = ch.chars();
            let c = it.next()?;
            it.next().is_none().then_some(Key::Char(c))
        }
        1 => Some(Key::Escape),
        2 => Some(Key::Tab),
        3 => Some(Key::Enter),
        4 => Some(Key::Backspace),
        5 => Some(Key::Delete),
        6 => Some(Key::Left),
        7 => Some(Key::Right),
        8 => Some(Key::Up),
        9 => Some(Key::Down),
        10 => Some(Key::Home),
        11 => Some(Key::End),
        20 => Some(Key::Ctrl),
        21 => Some(Key::Alt),
        22 => Some(Key::Shift),
        _ => None,
    }
}

/// Evento de entrada wire → modelo de inyección.
/// `None` = código/carácter no soportado (el receptor lo audita como denegado).
pub fn input_event_from_wire(m: &proto::InputEventMsg) -> Option<anyremote_input::InputEvent> {
    use anyremote_input::{InputEvent, MouseButton};
    match m.ev.as_ref()? {
        proto::input_event_msg::Ev::Key(k) => {
            let key = key_from_code(k.code, &k.ch)?;
            Some(if k.down {
                InputEvent::KeyDown(key)
            } else {
                InputEvent::KeyUp(key)
            })
        }
        proto::input_event_msg::Ev::Pointer(p) => Some(if p.absolute {
            // Absoluta: normalizada 0..65535 (coherente con el backend win32).
            InputEvent::MoveTo {
                x: p.x.clamp(0, u16::MAX as i32) as u16,
                y: p.y.clamp(0, u16::MAX as i32) as u16,
            }
        } else {
            InputEvent::MoveBy { dx: p.x, dy: p.y }
        }),
        proto::input_event_msg::Ev::Button(b) => {
            let button = match b.button {
                0 => MouseButton::Left,
                1 => MouseButton::Right,
                _ => MouseButton::Middle,
            };
            if b.wheel != 0 {
                Some(InputEvent::Scroll { delta: b.wheel })
            } else if b.down {
                Some(InputEvent::ButtonDown(button))
            } else {
                Some(InputEvent::ButtonUp(button))
            }
        }
    }
}

/// Frame de audio codificado → wire (la sesión fija Opus 48 kHz mono).
pub fn audio_frame_to_wire(frame: &anyremote_audio::EncodedAudio) -> proto::AudioFrame {
    proto::AudioFrame {
        seq: frame.seq,
        sample_rate: anyremote_audio::AUDIO_RATE,
        channels: 1,
        format: proto::AudioCodecEnum::AudioOpus as i32,
        data: frame.data.clone(),
    }
}

/// Wire → frame de audio validado contra el formato fijo de la sesión.
pub fn audio_frame_from_wire(
    v: proto::AudioFrame,
) -> Result<anyremote_audio::EncodedAudio, TransportError> {
    if v.sample_rate != anyremote_audio::AUDIO_RATE || v.channels != 1 {
        return Err(TransportError::Io(format!(
            "audio: se esperaba {} Hz mono; llegó {} Hz {} canales",
            anyremote_audio::AUDIO_RATE,
            v.sample_rate,
            v.channels
        )));
    }
    if v.format != proto::AudioCodecEnum::AudioOpus as i32 {
        return Err(TransportError::Io(format!(
            "audio: codec desconocido {}",
            v.format
        )));
    }
    if v.data.is_empty() {
        return Err(TransportError::Io("audio: frame vacío".into()));
    }
    Ok(anyremote_audio::EncodedAudio {
        seq: v.seq,
        data: v.data,
    })
}

/// Oferta de archivo → wire (nombre ya saneado por el emisor).
pub fn file_offer_to_wire(meta: &anyremote_filexfer::FileMeta) -> proto::FileOffer {
    proto::FileOffer {
        transfer_id: meta.transfer_id,
        name: meta.name.clone(),
        size: meta.size,
        sha256: meta.sha256.to_vec(),
    }
}

/// Wire → oferta de archivo con validación (nombre saneado, digest de 32 bytes).
pub fn file_offer_from_wire(v: proto::FileOffer) -> Result<anyremote_filexfer::FileMeta, TransportError> {
    let name = anyremote_filexfer::sanitize_name(&v.name)
        .map_err(|e| TransportError::Wire(format!("nombre de oferta: {e}")))?;
    let sha: [u8; 32] = v
        .sha256
        .as_slice()
        .try_into()
        .map_err(|n| TransportError::Wire(format!("sha256 con {n} bytes (se esperaban 32)")))?;
    Ok(anyremote_filexfer::FileMeta {
        transfer_id: v.transfer_id,
        name,
        size: v.size,
        sha256: sha,
    })
}

/// Chunk de archivo → wire.
pub fn file_chunk_to_wire(transfer_id: u64, offset: u64, data: &[u8]) -> proto::FileChunk {
    proto::FileChunk {
        transfer_id,
        offset,
        data: data.to_vec(),
    }
}

/// Resultado de transferencia → wire (`detail`: hash hex o `resume:{offset}` / error).
pub fn file_done_to_wire(transfer_id: u64, ok: bool, detail: &str) -> proto::FileDone {
    proto::FileDone {
        transfer_id,
        ok,
        detail: detail.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_check_accepts_current_and_rejects_future() {
        let ok = proto::Envelope {
            seq: 1,
            payload: Some(proto::envelope::Payload::Hello(proto::Hello {
                proto_version: ARSP_VERSION,
                node_id: "AAAA2222BB".into(),
                session_hint: String::new(),
            })),
        };
        assert!(check_version(&ok).is_ok());

        let bad = proto::Envelope {
            seq: 1,
            payload: Some(proto::envelope::Payload::Welcome(proto::Welcome {
                proto_version: ARSP_VERSION + 99,
                node_id: "AAAA2222BB".into(),
                capabilities: None,
            })),
        };
        assert!(check_version(&bad).is_err());
    }

    #[tokio::test]
    async fn envelope_roundtrip_over_duplex() {
        let (a, b) = tokio::io::duplex(4096);
        let mut tx = Channel::new(a);
        let mut rx = Channel::new(b);

        tx.send(hello("AAAA2222BB", "roundtrip")).await.unwrap();
        let env = rx.recv().await.unwrap();
        assert_eq!(env.seq, 1);
        assert_eq!(announced_node_id(&env).as_deref(), Some("AAAA2222BB"));

        tx.send(welcome("CCCC4444DD")).await.unwrap();
        let env = rx.recv().await.unwrap();
        assert_eq!(env.seq, 2);
        assert!(check_version(&env).is_ok());
    }

    /// RF-02a: el wire de entrada mapea al modelo de inyección (tabla de `Key`).
    #[test]
    fn input_wire_mapping_covers_keys_pointer_buttons() {
        use anyremote_input::{InputEvent, Key, MouseButton};

        let key = |code: u32, ch: &str, down: bool| proto::InputEventMsg {
            ev: Some(proto::input_event_msg::Ev::Key(proto::Key {
                code,
                down,
                extended: false,
                ch: ch.into(),
            })),
        };
        assert_eq!(
            input_event_from_wire(&key(0, "a", true)),
            Some(InputEvent::KeyDown(Key::Char('a')))
        );
        assert_eq!(
            input_event_from_wire(&key(0, "A", false)),
            Some(InputEvent::KeyUp(Key::Char('A')))
        );
        assert_eq!(
            input_event_from_wire(&key(1, "", true)),
            Some(InputEvent::KeyDown(Key::Escape))
        );
        assert_eq!(
            input_event_from_wire(&key(22, "", true)),
            Some(InputEvent::KeyDown(Key::Shift))
        );
        // Código desconocido, carácter vacío o multibyte → no soportado.
        assert_eq!(input_event_from_wire(&key(99, "", true)), None);
        assert_eq!(input_event_from_wire(&key(0, "", true)), None);
        assert_eq!(input_event_from_wire(&key(0, "ab", true)), None);

        let pointer = |x: i32, y: i32, absolute: bool| proto::InputEventMsg {
            ev: Some(proto::input_event_msg::Ev::Pointer(proto::Pointer {
                x,
                y,
                absolute,
            })),
        };
        assert_eq!(
            input_event_from_wire(&pointer(320, 180, true)),
            Some(InputEvent::MoveTo { x: 320, y: 180 })
        );
        assert_eq!(
            input_event_from_wire(&pointer(-4, 7, false)),
            Some(InputEvent::MoveBy { dx: -4, dy: 7 })
        );
        // Fuera de rango de pantalla → recortado a las dimensiones u16 del modelo.
        assert_eq!(
            input_event_from_wire(&pointer(70_000, -5, true)),
            Some(InputEvent::MoveTo {
                x: u16::MAX,
                y: 0
            })
        );

        let button = |button: u32, down: bool, wheel: i32| proto::InputEventMsg {
            ev: Some(proto::input_event_msg::Ev::Button(proto::Button {
                button,
                down,
                wheel,
            })),
        };
        assert_eq!(
            input_event_from_wire(&button(0, true, 0)),
            Some(InputEvent::ButtonDown(MouseButton::Left))
        );
        assert_eq!(
            input_event_from_wire(&button(1, false, 0)),
            Some(InputEvent::ButtonUp(MouseButton::Right))
        );
        assert_eq!(
            input_event_from_wire(&button(0, false, -3)),
            Some(InputEvent::Scroll { delta: -3 })
        );
        // oneof vacío → no soportado.
        assert_eq!(input_event_from_wire(&proto::InputEventMsg { ev: None }), None);
    }

    /// RF audio: el wire valida el formato fijo de sesión (Opus 48 kHz mono).
    #[test]
    fn audio_frame_wire_roundtrip_and_validation() {
        let wire = audio_frame_to_wire(&anyremote_audio::EncodedAudio {
            seq: 7,
            data: vec![1, 2, 3],
        });
        assert_eq!(wire.sample_rate, anyremote_audio::AUDIO_RATE);
        assert_eq!(wire.channels, 1);
        assert_eq!(wire.format, proto::AudioCodecEnum::AudioOpus as i32);
        let back = audio_frame_from_wire(wire).expect("válido");
        assert_eq!(back.seq, 7);
        assert_eq!(back.data, vec![1, 2, 3]);

        let mut bad = proto::AudioFrame {
            seq: 1,
            sample_rate: 44_100,
            channels: 1,
            format: 0,
            data: vec![9],
        };
        assert!(audio_frame_from_wire(bad.clone()).is_err(), "tasa ajena");
        bad.sample_rate = anyremote_audio::AUDIO_RATE;
        bad.channels = 2;
        assert!(audio_frame_from_wire(bad.clone()).is_err(), "estéreo");
        bad.channels = 1;
        bad.data = Vec::new();
        assert!(audio_frame_from_wire(bad).is_err(), "frame vacío");
    }
}
