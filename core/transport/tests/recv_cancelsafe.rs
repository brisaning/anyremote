//! Regresión v0.1.6 — causa raíz del `FrameTooLarge(134967817)` en la prueba
//! cross-device: `Channel::recv()` cancelado a mitad por otra rama de
//! `tokio::select!` perdia los bytes parciales → desincronizacion → muerte.
//!
//! Este test cancela la recepcion constantemente (tick de 1 ms) mientras llegan
//! frames grandes que inevitablemente se leen a trozos: con la version antigua
//! se perdian bytes (secuencias fuera de orden / FrameTooLarge / EOF); con el
//! buffer persistente se reciben los 10 completos y en orden.

use std::time::Duration;

use anyremote_transport::arsp::{self, proto};
use tokio::io::DuplexStream;

fn big_hello(i: usize) -> proto::envelope::Payload {
    arsp::hello("NODETEST12", &format!("hint-grande-{i}-{}", "x".repeat(3000)))
}

#[tokio::test]
async fn recv_survives_constant_select_cancellation() {
    // Buffer diminuto: fuerza lecturas parciales con suspensión (Pending) a mitad
    // de frame → la ventana de cancelación es real (reproducia el bug en vivo).
    let (a, b) = tokio::io::duplex(512);
    let mut tx = arsp::Channel::<DuplexStream>::new(a);
    let mut rx = arsp::Channel::<DuplexStream>::new(b);

    let sender = tokio::spawn(async move {
        for i in 0..10usize {
            tx.send(big_hello(i)).await.expect("send");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });

    let mut tick = tokio::time::interval(Duration::from_millis(1));
    let mut got: Vec<u64> = Vec::new();
    while got.len() < 10 {
        tokio::select! {
            // Cada iteracion puede CANCELAR esta recepcion a mitad de frame.
            env = rx.recv() => {
                let env = env.expect("recv sin error (bytes no perdidos)");
                match env.payload {
                    Some(proto::envelope::Payload::Hello(h)) => {
                        assert!(h.session_hint.starts_with("hint-grande-"), "payload integro");
                        assert_eq!(h.node_id, "NODETEST12");
                    }
                    other => panic!("payload inesperado: {other:?}"),
                }
                got.push(env.seq);
            }
            _ = tick.tick() => {
                // rama que roba la ejecucion y cancela la recepcion
            }
        }
    }

    assert_eq!(got, (1..=10).collect::<Vec<_>>(), "secuencias completas y en orden");
    sender.await.expect("emisor");
}
