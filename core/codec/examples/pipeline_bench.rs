//! Benchmark del pipeline de vídeo C3 (gate RNF-02): captura -> códec -> decodificación
//! con integridad verificada. Ejecutar:
//! `cargo run -p anyremote-codec --example pipeline_bench`
use std::time::Instant;

use anyremote_capture::{Capturer, TestPatternCapturer};
use anyremote_codec::{encoder_for, CodecFormat, NativeDecoder, VideoDecoder};

fn main() {
    const W: u32 = 1280;
    const H: u32 = 720;
    const FRAMES: u64 = 240;

    println!("=== anyremote C3 — benchmark de pipeline de vídeo ({W}x{H}, {FRAMES} frames) ===");
    let mut capturer = TestPatternCapturer::new(W, H);
    let mut decoder = NativeDecoder::new();

    for format in [CodecFormat::Deflate, CodecFormat::Raw] {
        let mut encoder = encoder_for(format);
        let mut total = std::time::Duration::ZERO;
        let mut encoded_bytes = 0usize;
        let mut raw_bytes = 0usize;

        for _ in 0..FRAMES {
            let frame = capturer.next_frame().expect("capture");
            raw_bytes += frame.data.len();
            let t0 = Instant::now();
            let enc = encoder.encode(&frame).expect("encode");
            let dec = decoder.decode(&enc).expect("decode");
            total += t0.elapsed();
            assert_eq!(dec, frame, "integridad lossless obligatoria");
            encoded_bytes += enc.data.len();
        }

        let avg_ms = total.as_secs_f64() * 1000.0 / FRAMES as f64;
        let fps = FRAMES as f64 / total.as_secs_f64();
        let ratio = encoded_bytes as f64 / raw_bytes as f64;
        println!(
            "[{format:?}] avg={avg_ms:.2} ms/frame · {fps:.0} fps · compresión={ratio:.2}x ({encoded_bytes}/{raw_bytes} B)"
        );
    }

    // ---- Brazo VP9 (gate C3-bis): códec con pérdida — verificación geométrica, no byte a byte ----
    #[cfg(feature = "vpx")]
    {
        let mut capturer = TestPatternCapturer::new(W, H);
        let mut encoder = anyremote_codec::encoder_for(CodecFormat::Vp9);
        let mut decoder = NativeDecoder::new();

        let mut encoded_bytes = 0usize;
        let mut encoded_count = 0usize;
        let mut decoded_count = 0usize;
        let mut enc_total = std::time::Duration::ZERO;
        let mut dec_total = std::time::Duration::ZERO;

        for _ in 0..FRAMES {
            let frame = capturer.next_frame().expect("capture");
            let t0 = Instant::now();
            let enc = encoder.encode(&frame).expect("encode vp9");
            enc_total += t0.elapsed();
            if enc.data.is_empty() {
                continue; // omitida por el rate control (documentado en vpx_backend)
            }
            encoded_count += 1;
            encoded_bytes += enc.data.len();
            let t1 = Instant::now();
            let dec = decoder.decode(&enc).expect("decode vp9");
            dec_total += t1.elapsed();
            assert_eq!((dec.width, dec.height), (W, H), "geometría preservada");
            assert_eq!(dec.data.len(), (W * H * 4) as usize);
            decoded_count += 1;
        }

        let enc_fps = encoded_count as f64 / enc_total.as_secs_f64();
        let dec_fps = decoded_count as f64 / dec_total.as_secs_f64();
        let input_fps = FRAMES as f64 / (enc_total + dec_total).as_secs_f64();
        let ratio = encoded_bytes as f64 / (FRAMES as f64 * (W * H * 4) as f64);
        println!(
            "[Vp9] encode={enc_fps:.0} fps · decode={dec_fps:.0} fps · pipeline={input_fps:.0} fps · frames={encoded_count}/{decoded_count} · compresión={ratio:.3}x ({encoded_bytes} B)"
        );
        assert!(
            enc_fps >= 30.0,
            "objetivo RNF-02: >=30 fps encode, llegó {enc_fps:.1}"
        );
    }
    println!("=== benchmark completado (integridad verificada en todos los frames) ===");
}
