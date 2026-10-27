//! Gate del ciclo C3 (RF-02a): streaming de vídeo ARSP (captura -> códec -> sesión TLS)
//! y eventos de entrada de vuelta al lado controlado, con métricas de FPS.

use std::sync::Arc;
use std::time::Instant;

use anyremote_capture::{Capturer, TestPatternCapturer};
use anyremote_codec::{NativeDecoder, VideoDecoder};
use anyremote_input::{InputEvent, InputInjector, Key, MouseButton, RecordingInjector};
use anyremote_session::{SessionEvent, SessionState};
use anyremote_transport::arsp::{self, proto};
use anyremote_transport::{client_config, generate_loopback_cert, server_config};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

const W: u32 = 640;
const H: u32 = 360;
const FRAMES: u64 = 30;

#[tokio::test]
async fn e2e_video_streaming_and_input_injection() {
    let cert = generate_loopback_cert().expect("cert");
    let srv_cfg = server_config(&cert).expect("cfg");
    let cli_cfg = client_config(&cert.cert).expect("cfg");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();

    // ---- Lado controlado: captura -> encode -> envía frames; recibe input ----
    let controlled = tokio::spawn(async move {
        let acceptor = TlsAcceptor::from(Arc::new(srv_cfg));
        let (tcp, _) = listener.accept().await.expect("accept");
        let mut ch = arsp::Channel::new(acceptor.accept(tcp).await.expect("tls"));

        let mut capturer = TestPatternCapturer::new(W, H);
        let mut encoder = anyremote_codec::encoder_for(anyremote_codec::CodecFormat::Deflate);
        for _ in 0..FRAMES {
            let frame = capturer.next_frame().expect("capture");
            let enc = encoder.encode(&frame).expect("encode");
            ch.send(proto::envelope::Payload::VideoFrame(proto::VideoFrame {
                seq: enc.seq,
                width: enc.width,
                height: enc.height,
                format: enc.format as i32,
                data: enc.data,
            }))
            .await
            .expect("enviar frame");
        }

        // Recibe y registra3 eventos de entrada (RecordingInjector: no toca el sistema).
        let mut injector = RecordingInjector::new();
        for _ in 0..3 {
            let env = ch.recv().await.expect("input msg");
            if let Some(proto::envelope::Payload::InputEvent(m)) = env.payload {
                let ev = arsp::input_event_from_wire(&m).expect("mapeo");
                injector.inject(&ev).expect("inyectar");
            } else {
                panic!("payload inesperado");
            }
        }
        injector.events
    });

    // ---- Lado controlador: recibe frames, decodifica y verifica ----
    let tcp = TcpStream::connect(addr).await.expect("connect");
    let sni = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let mut ch = arsp::Channel::new(
        TlsConnector::from(Arc::new(cli_cfg))
            .connect(sni, tcp)
            .await
            .expect("tls"),
    );

    let mut state = SessionState::Handshaking;
    state = state.transition(SessionEvent::HandshakeOk).unwrap();
    state = state.transition(SessionEvent::PermissionGranted).unwrap();

    let mut decoder = NativeDecoder::new();
    let start = Instant::now();
    for i in 0..FRAMES {
        let env = ch.recv().await.expect("frame");
        let f = match env.payload {
            Some(proto::envelope::Payload::VideoFrame(v)) => v,
            _ => panic!("payload inesperado"),
        };
        assert_eq!((f.width, f.height), (W, H));
        let decoded = decoder
            .decode(&anyremote_codec::EncodedFrame {
                format: anyremote_codec::CodecFormat::from_proto(f.format).expect("formato"),
                seq: f.seq,
                width: f.width,
                height: f.height,
                data: f.data,
            })
            .expect("decode");
        if i == 0 {
            // Integridad: el primer frame debe ser idéntico al patrón determinista.
            assert_eq!(decoded, TestPatternCapturer::render(W, H, 0));
        }
    }
    let elapsed = start.elapsed();
    let fps = FRAMES as f64 / elapsed.as_secs_f64();
    assert!(fps > 0.0);

    // Envía eventos de entrada al controlado.
    for ev in [
        proto::envelope::Payload::InputEvent(proto::InputEventMsg {
            ev: Some(proto::input_event_msg::Ev::Key(proto::Key {
                code: 0,
                down: true,
                extended: false,
                ch: "a".into(),
            })),
        }),
        proto::envelope::Payload::InputEvent(proto::InputEventMsg {
            ev: Some(proto::input_event_msg::Ev::Pointer(proto::Pointer {
                x: 320,
                y: 180,
                absolute: true,
            })),
        }),
        proto::envelope::Payload::InputEvent(proto::InputEventMsg {
            ev: Some(proto::input_event_msg::Ev::Button(proto::Button {
                button: 0,
                down: true,
                wheel: 0,
            })),
        }),
    ] {
        ch.send(ev).await.expect("enviar input");
    }

    let recorded = controlled.await.expect("tarea controlado");
    assert_eq!(
        recorded,
        vec![
            InputEvent::KeyDown(Key::Char('a')),
            InputEvent::MoveTo { x: 320, y: 180 },
            InputEvent::ButtonDown(MouseButton::Left),
        ]
    );

    state = state.transition(SessionEvent::StartClose).unwrap();
    assert_eq!(state.transition(SessionEvent::Closed).unwrap(), SessionState::Closed);
}
