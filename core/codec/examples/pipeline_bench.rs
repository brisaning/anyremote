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
    println!("=== benchmark completado (integridad verificada en todos los frames) ===");
}
