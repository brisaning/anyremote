//! Gate del ciclo C4 (RF-02b): sincronización bidireccional de portapapeles sobre
//! la sesión ARSP con **contención de formatos** (solo texto) y supresión de eco.

use std::sync::Arc;

use anyremote_clipboard::{ClipboardSyncEngine, MemoryClipboard};
use anyremote_transport::arsp::{self, proto};
use anyremote_transport::{client_config, generate_loopback_cert, server_config, TransportError};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

#[tokio::test]
async fn e2e_clipboard_bidirectional_with_format_containment() {
    let cert = generate_loopback_cert().expect("cert");
    let srv_cfg = server_config(&cert).expect("cfg");
    let cli_cfg = client_config(&cert.cert).expect("cfg");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();

    // ---- Controlado: motor con backend en memoria ----
    let controlled = tokio::spawn(async move {
        let acceptor = TlsAcceptor::from(Arc::new(srv_cfg));
        let (tcp, _) = listener.accept().await.expect("accept");
        let mut ch = arsp::Channel::new(acceptor.accept(tcp).await.expect("tls"));
        let mut eng = ClipboardSyncEngine::new(MemoryClipboard::with_text("saludo del controlado"));

        // 1) Envía su cambio local inicial.
        let out = eng
            .detect_local_change()
            .expect("detect 1")
            .expect("hay cambio");
        ch.send(proto::envelope::Payload::Clipboard(
            arsp::clipboard_to_wire(&out).expect("wire"),
        ))
        .await
        .expect("enviar saludo");

        // 2) Recibe el texto del controlador y lo aplica.
        let env = ch.recv().await.expect("recv 1");
        let wire = match env.payload {
            Some(proto::envelope::Payload::Clipboard(w)) => w,
            _ => panic!("payload inesperado"),
        };
        let content = arsp::clipboard_from_wire(wire).expect("from wire");
        assert!(eng.apply_remote(&content).expect("aplicar"));
        assert_eq!(
            eng.backend().current.as_deref(),
            Some("hola desde controlador")
        );

        // 3) Tras aplicar, detectar NO debe reenviar (eco suprimido).
        assert!(eng.detect_local_change().expect("detect 2").is_none());
        assert!(eng.stats.echo_suppressed >= 1);

        // 4) Cambio local real del controlado → envía.
        eng.backend_mut().current = Some("respuesta del controlado".into());
        let out2 = eng
            .detect_local_change()
            .expect("detect 3")
            .expect("cambio 2");
        ch.send(proto::envelope::Payload::Clipboard(
            arsp::clipboard_to_wire(&out2).expect("wire"),
        ))
        .await
        .expect("enviar respuesta");

        // 5) El controlador envía un formato NO admitido → rechazado en la frontera sin tocar el backend.
        let env2 = ch.recv().await.expect("recv 2");
        match env2.payload {
            Some(proto::envelope::Payload::Clipboard(w)) => {
                assert!(
                    arsp::clipboard_from_wire(w).is_err(),
                    "contención: formato no admitido rechazado en frontera"
                );
                assert_eq!(
                    eng.backend().current.as_deref(),
                    Some("respuesta del controlado"),
                    "el backend queda intacto tras el rechazo"
                );
            }
            _ => panic!("payload 2 inesperado"),
        }

        eng.backend().current.clone().unwrap_or_default()
    });

    // ---- Controlador ----
    let tcp = TcpStream::connect(addr).await.expect("connect");
    let sni = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let mut ch = arsp::Channel::new(
        TlsConnector::from(Arc::new(cli_cfg))
            .connect(sni, tcp)
            .await
            .expect("tls"),
    );

    let mut eng = ClipboardSyncEngine::new(MemoryClipboard::default());
    // 1) Recibe el saludo del controlado.
    let env = ch.recv().await.expect("recv saludo");
    let wire = match env.payload {
        Some(proto::envelope::Payload::Clipboard(w)) => w,
        _ => panic!("payload inesperado"),
    };
    let content = arsp::clipboard_from_wire(wire).expect("from wire");
    assert!(eng.apply_remote(&content).expect("aplicar saludo"));
    assert_eq!(
        eng.backend().current.as_deref(),
        Some("saludo del controlado")
    );

    // 2) Cambio local del controlador → envía.
    eng.backend_mut().current = Some("hola desde controlador".into());
    let out = eng.detect_local_change().expect("detect").expect("cambio");
    ch.send(proto::envelope::Payload::Clipboard(
        arsp::clipboard_to_wire(&out).expect("wire"),
    ))
    .await
    .expect("enviar hola");

    // 3) Recibe la respuesta y la aplica (bidireccionalidad verificada).
    let env2 = ch.recv().await.expect("recv respuesta");
    let wire2 = match env2.payload {
        Some(proto::envelope::Payload::Clipboard(w)) => w,
        _ => panic!("payload 2 inesperado"),
    };
    let content2 = arsp::clipboard_from_wire(wire2).expect("from wire 2");
    assert!(eng.apply_remote(&content2).expect("aplicar respuesta"));
    assert_eq!(
        eng.backend().current.as_deref(),
        Some("respuesta del controlado")
    );

    // 4) Contención: envía un formato NO admitido (wire format=7) al controlado.
    let bad_wire = proto::ClipboardSync {
        revision: 50,
        format: 7,
        utf8: b"imagen".to_vec(),
    };
    ch.send(proto::envelope::Payload::Clipboard(bad_wire))
        .await
        .expect("enviar formato no admitido");
    // Rechazo también verificado localmente en la frontera (defensa en profundidad).
    let local_bad = proto::ClipboardSync {
        revision: 51,
        format: 9,
        utf8: b"otro".to_vec(),
    };
    let err = arsp::clipboard_from_wire(local_bad).expect_err("debe rechazar formato");
    assert!(
        matches!(err, TransportError::Auth(_)),
        "error inesperado: {err:?}"
    );

    let final_controlled = controlled.await.expect("tarea controlado");
    assert_eq!(final_controlled, "respuesta del controlado");
    // Ambos lados coinciden (sincronización bidireccional completa).
    assert_eq!(
        eng.backend().current.as_deref(),
        Some("respuesta del controlado")
    );
}
