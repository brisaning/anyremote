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
}
