//! Servidor de previsualización local (loopback): frames BGRA crudos con cabecera.
//!
//! Formato de stream (little-endian): `u32 width | u32 height | u32 seq | u32 len | len bytes`.
//! Un solo cliente a la vez; si no hay cliente o el buffer está lleno, los frames
//! se descartan (backpressure de visualización, no de sesión).

use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

pub struct PreviewServer {
    tx: mpsc::Sender<Vec<u8>>,
}

impl PreviewServer {
    /// Bind efímero en 127.0.0.1. Devuelve (servidor, puerto, tarea).
    pub fn start() -> (Self, u16, tokio::task::JoinHandle<()>) {
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("preview bind");
        let port = std_listener.local_addr().expect("addr").port();
        std_listener.set_nonblocking(true).expect("nonblocking");
        let listener = TcpListener::from_std(std_listener).expect("tokio listener");
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(3);
        let handle = tokio::spawn(async move {
            let mut client: Option<tokio::net::TcpStream> = None;
            loop {
                tokio::select! {
                    accepted = listener.accept(), if client.is_none() => {
                        match accepted {
                            Ok((sock, _)) => client = Some(sock),
                            Err(_) => break,
                        }
                    }
                    batch = rx.recv(), if client.is_some() => {
                        let Some(batch) = batch else { break };
                        let sock = client.as_mut().expect("cliente");
                        if sock.write_all(&batch).await.is_err() {
                            client = None; // el visor se desconectó: espera a otro
                        }
                    }
                }
            }
        });
        (Self { tx }, port, handle)
    }

    /// Encola un frame (descarte silencioso si no hay cliente o hay backpressure).
    pub fn push(&self, width: u32, height: u32, seq: u32, bgra: &[u8]) -> bool {
        if bgra.len() > 64 * 1024 * 1024 {
            return false;
        }
        let mut pkt = Vec::with_capacity(16 + bgra.len());
        pkt.extend_from_slice(&width.to_le_bytes());
        pkt.extend_from_slice(&height.to_le_bytes());
        pkt.extend_from_slice(&seq.to_le_bytes());
        pkt.extend_from_slice(&(bgra.len() as u32).to_le_bytes());
        pkt.extend_from_slice(bgra);
        self.tx.try_send(pkt).is_ok()
    }
}
