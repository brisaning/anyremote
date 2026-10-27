//! Gate del ciclo C3-bis: streaming VP9 real sobre la sesión ARSP (RF-02a + RNF-02).
//! Se compila solo con `--features vpx`.

#![cfg(feature = "vpx")]

use std::sync::Arc;

use anyremote_capture::{Capturer, TestPatternCapturer};
use anyremote_codec::{encoder_for, CodecFormat, NativeDecoder, VideoDecoder};
use anyremote_transport::arsp::{self, proto};
use anyremote_transport::{client_config, generate_loopback_cert, server_config};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

const W: u32 = 320;
const H: u32 = 176;
const ATTEMPTS: u64 = 120;

#[tokio::test]
async fn e2e_vp9_streaming_over_session() {
    let cert = generate_loopback_cert().expect("cert");
    let srv_cfg = server_config(&cert).expect("cfg");
    let cli_cfg = client_config(&cert.cert).expect("cfg");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();

    // ---- Controlado: captura -> VP9 -> wire ----
    let controlled = tokio::spawn(async move {
        let acceptor = TlsAcceptor::from(Arc::new(srv_cfg));
        let (tcp, _) = listener.accept().await.expect("accept");
        let mut ch = arsp::Channel::new(acceptor.accept(tcp).await.expect("tls"));
        let mut capturer = TestPatternCapturer::new(W, H);
        let mut encoder = encoder_for(CodecFormat::Vp9);
        let mut sent = 0u64;
        for _ in 0..ATTEMPTS {
            let frame = capturer.next_frame().expect("capture");
            let enc = encoder.encode(&frame).expect("encode vp9");
            // Se envían también las omitidas (data vacía) para determinismo del conteo.
            ch.send(proto::envelope::Payload::VideoFrame(
                arsp::video_frame_to_wire(&enc),
            ))
            .await
            .expect("enviar frame vp9");
            if !enc.data.is_empty() {
                sent += 1;
            }
        }
        sent
    });

    // ---- Controlador: recibe -> decodifica ----
    let tcp = TcpStream::connect(addr).await.expect("connect");
    let sni = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let mut ch = arsp::Channel::new(
        TlsConnector::from(Arc::new(cli_cfg))
            .connect(sni, tcp)
            .await
            .expect("tls"),
    );

    let mut decoder = NativeDecoder::new();
    let mut received_nonempty = 0u64;
    let mut decoded = 0u64;
    for _ in 0..ATTEMPTS {
        let env = ch.recv().await.expect("frame");
        let wire = match env.payload {
            Some(proto::envelope::Payload::VideoFrame(v)) => v,
            _ => panic!("payload inesperado"),
        };
        let enc = arsp::video_frame_from_wire(wire).expect("wire -> enc");
        if enc.data.is_empty() {
            continue;
        }
        received_nonempty += 1;
        let dec = decoder.decode(&enc).expect("decode vp9");
        assert_eq!((dec.width, dec.height), (W, H));
        decoded += 1;
    }

    let sent = controlled.await.expect("tarea controlado");
    assert_eq!(sent, received_nonempty, "conteo enviado/recibido coherente");
    assert_eq!(decoded, received_nonempty, "lockstep decodificación");
    assert!(
        decoded >= 100,
        "tasa de frames codificados demasiado baja: {decoded}/{ATTEMPTS}"
    );
}
