//! Gate del ciclo C5 (RF-02c): transferencia de archivos sobre la sesión ARSP con
//! integridad SHA-256, reanudación desde el offset confirmado, política de límites
//! y rechazo de nombres con traversal.

use std::sync::Arc;

use anyremote_filexfer::{sha256_hex, FileReceiver, FileSender, TransferPolicy};
use anyremote_transport::arsp::{self, proto};
use anyremote_transport::{client_config, generate_loopback_cert, server_config};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

const CHUNK: usize = 32 * 1024;

#[tokio::test]
async fn e2e_filexfer_hash_integrity_and_resume() {
    let cert = generate_loopback_cert().expect("cert");
    let srv_cfg = server_config(&cert).expect("cfg");
    let cli_cfg = client_config(&cert.cert).expect("cfg");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();

    let data: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
    let data_hash = sha256_hex(&data);

    // ---- Controlado: receptor con política de límites ----
    let controlled = tokio::spawn({
        let data_hash = data_hash.clone();
        async move {
            let acceptor = TlsAcceptor::from(Arc::new(srv_cfg));
            let (tcp, _) = listener.accept().await.expect("accept");
            let mut ch = arsp::Channel::new(acceptor.accept(tcp).await.expect("tls"));
            let mut recv = FileReceiver::new(TransferPolicy {
                max_bytes_per_file: 128 * 1024,
                ..Default::default()
            });

            // 1) Oferta válida.
            let env = ch.recv().await.expect("offer");
            let offer = match env.payload {
                Some(proto::envelope::Payload::FileOffer(o)) => o,
                _ => panic!("payload inesperado"),
            };
            let meta = arsp::file_offer_from_wire(offer).expect("meta");
            recv.accept_offer(meta).expect("aceptar oferta");

            // 2) Chunk 0 OK.
            let env0 = ch.recv().await.expect("chunk0");
            if let Some(proto::envelope::Payload::FileChunk(c)) = env0.payload {
                let next = recv
                    .write_chunk(c.transfer_id, c.offset, &c.data)
                    .expect("chunk0");
                assert_eq!(next as usize, CHUNK.min(100_000));
            } else {
                panic!("esperaba chunk");
            }

            // 3) Chunk con hueco → Offset{expected} → reanudación por detalle "resume:N".
            let env_gap = ch.recv().await.expect("chunk gap");
            if let Some(proto::envelope::Payload::FileChunk(c)) = env_gap.payload {
                match recv.write_chunk(c.transfer_id, c.offset, &c.data) {
                    Err(anyremote_filexfer::TransferError::Offset { expected, got }) => {
                        assert_eq!(expected as usize, CHUNK);
                        assert!(got > expected);
                        ch.send(proto::envelope::Payload::FileDone(arsp::file_done_to_wire(
                            c.transfer_id,
                            false,
                            &format!("resume:{expected}"),
                        )))
                        .await
                        .expect("enviar resume");
                    }
                    other => panic!("esperaba Offset, llegó {other:?}"),
                }
            } else {
                panic!("esperaba chunk");
            }

            // 4) Recibe el resto reanudado, verifica hash y lo escribe a disco real.
            let mut resumed_ok = false;
            while recv.next_offset(99).unwrap() < 100_000 {
                let env = ch.recv().await.expect("chunk resumido");
                if let Some(proto::envelope::Payload::FileChunk(c)) = env.payload {
                    recv.write_chunk(c.transfer_id, c.offset, &c.data)
                        .expect("chunk reanudado");
                    resumed_ok = true;
                } else {
                    panic!("esperaba chunk");
                }
            }
            assert!(resumed_ok, "la reanudación debe entregar chunks");

            let dir = std::env::temp_dir().join(format!("anyremote-c5-e2e-{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("tempdir");
            let path = recv.finish_to_file(99, &dir).expect("finish con hash");
            let read_back = std::fs::read(&path).expect("leer archivo");
            assert_eq!(read_back.len(), 100_000);
            assert_eq!(sha256_hex(&read_back), data_hash);

            ch.send(proto::envelope::Payload::FileDone(arsp::file_done_to_wire(
                99,
                true,
                &sha256_hex(&read_back),
            )))
            .await
            .expect("done ok");
            let _ = std::fs::remove_dir_all(&dir);

            // 5) Oferta con nombre traversal → rechazo en receptor.
            let env_evil = ch.recv().await.expect("evil offer");
            if let Some(proto::envelope::Payload::FileOffer(o)) = env_evil.payload {
                match arsp::file_offer_from_wire(o) {
                    Ok(meta) => {
                        let err = recv.accept_offer(meta).expect_err("debe rechazar");
                        let detail = format!("{err:?}");
                        ch.send(proto::envelope::Payload::FileDone(arsp::file_done_to_wire(
                            100, false, &detail,
                        )))
                        .await
                        .expect("done fail");
                        assert!(
                            detail.contains("InvalidName") || detail.contains("TooLarge"),
                            "detalle: {detail}"
                        );
                    }
                    Err(e) => panic!("la frontera debió rechazar primero: {e:?}"),
                }
            } else {
                panic!("esperaba offer");
            }
        }
    });

    // ---- Controlador: emisor con reanudación ----
    let tcp = TcpStream::connect(addr).await.expect("connect");
    let sni = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let mut ch = arsp::Channel::new(
        TlsConnector::from(Arc::new(cli_cfg))
            .connect(sni, tcp)
            .await
            .expect("tls"),
    );

    let sender = FileSender::new(TransferPolicy::default());
    let offer = sender.offer(99, "datos.bin", &data).expect("oferta");
    ch.send(proto::envelope::Payload::FileOffer(
        arsp::file_offer_to_wire(&offer),
    ))
    .await
    .expect("enviar offer");

    // Chunk 0 + chunk final con hueco deliberado (disparador de reanudación).
    let chunks: Vec<_> = sender.chunks_from(&data, 0, CHUNK).collect();
    ch.send(proto::envelope::Payload::FileChunk(
        arsp::file_chunk_to_wire(99, chunks[0].0, chunks[0].1),
    ))
    .await
    .expect("chunk0");
    let last = chunks.last().unwrap();
    ch.send(proto::envelope::Payload::FileChunk(
        arsp::file_chunk_to_wire(99, last.0, last.1),
    ))
    .await
    .expect("chunk con hueco");

    // Espera el pedido de reanudación y reenvía desde el offset confirmado.
    let env = ch.recv().await.expect("done resume");
    let done = match env.payload {
        Some(proto::envelope::Payload::FileDone(d)) => d,
        _ => panic!("esperaba FileDone"),
    };
    assert!(!done.ok, "debe pedir reanudación");
    let resume_from: u64 = done
        .detail
        .strip_prefix("resume:")
        .expect("formato resume:N")
        .parse()
        .expect("offset numérico");
    assert_eq!(resume_from as usize, CHUNK);

    for (off, chunk) in sender.chunks_from(&data, resume_from, CHUNK) {
        ch.send(proto::envelope::Payload::FileChunk(
            arsp::file_chunk_to_wire(99, off, chunk),
        ))
        .await
        .expect("chunk reenviado");
    }

    // Confirmación con hash del receptor.
    let env = ch.recv().await.expect("done final");
    let done = match env.payload {
        Some(proto::envelope::Payload::FileDone(d)) => d,
        _ => panic!("esperaba FileDone final"),
    };
    assert!(done.ok, "receptor debe confirmar: {}", done.detail);
    assert_eq!(
        done.detail, data_hash,
        "hash del receptor coincide con el original"
    );

    // Política del emisor: oferta con nombre inválido falla en origen.
    let evil_err = sender
        .offer(100, "../evil.bin", &data)
        .expect_err("nombre inválido");
    assert!(format!("{evil_err:?}").contains("InvalidName"));
    ch.send(proto::envelope::Payload::FileOffer(
        arsp::file_offer_to_wire(&anyremote_filexfer::FileMeta {
            transfer_id: 100,
            name: "grande.bin".into(),
            size: 200_000, // supera el max_bytes_per_file del receptor (128 KiB)
            sha256: [0u8; 32],
        }),
    ))
    .await
    .expect("segunda offer");
    let env = ch.recv().await.expect("done 2");
    let done2 = match env.payload {
        Some(proto::envelope::Payload::FileDone(d)) => d,
        _ => panic!("esperaba FileDone 2"),
    };
    assert!(
        !done2.ok,
        "límite del receptor debe rechazar: {}",
        done2.detail
    );
    assert!(
        done2.detail.contains("TooLarge"),
        "detalle: {}",
        done2.detail
    );

    controlled.await.expect("tarea controlado");
}
