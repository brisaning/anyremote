//! Sonda C-33 (ignorada en el gate): calidad real del pipeline completo
//! captura → escala → H.264 → decode con **contenido en movimiento**
//! (scroll), que es cuando el texto se degrada en campo.
//!
//! Ejecutar: `cargo test -p anyremote-daemon --test video_quality_probe --
//! --ignored --nocapture`.

use anyremote_capture::{initial_snapshot, Frame};
use anyremote_codec::{encoder_for, CodecFormat, NativeDecoder, VideoDecoder};
use anyremote_daemon::video::scale_frame;

fn luma(frame: &Frame) -> Vec<u8> {
    frame
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| ((66 * p[2] as u32 + 129 * p[1] as u32 + 25 * p[0] as u32 + 128) >> 8) as u8)
        .collect()
}

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let mse: f64 = a
        .iter()
        .zip(b)
        .map(|(&x, &y)| {
            let d = x as f64 - y as f64;
            d * d
        })
        .sum::<f64>()
        / a.len().max(1) as f64;
    if mse <= 0.0 {
        return 99.0;
    }
    10.0 * (255.0f64 * 255.0 / mse).log10()
}

/// Scroll horizontal envolvente (simula desplazar una ventana de texto).
fn scroll(src: &Frame, dx: u32) -> Frame {
    let (w, h) = (src.width as usize, src.height as usize);
    let dx = (dx as usize) % w;
    let mut data = vec![0u8; src.data.len()];
    for y in 0..h {
        let row = &src.data[y * w * 4..(y + 1) * w * 4];
        let dst = &mut data[y * w * 4..(y + 1) * w * 4];
        let split = dx * 4;
        dst[..row.len() - split].copy_from_slice(&row[split..]);
        dst[row.len() - split..].copy_from_slice(&row[..split]);
    }
    Frame {
        width: src.width,
        height: src.height,
        format: src.format,
        data,
    }
}

#[test]
#[ignore]
fn probe_scroll_quality() {
    let src = initial_snapshot().expect("snapshot del escritorio");
    let base = scale_frame(&src, 1920, 1080);
    eprintln!("escritorio: {}x{}", src.width, src.height);
    let frames: Vec<Frame> = (0..24).map(|i| scroll(&base, i * 3)).collect();
    let luma_src: Vec<Vec<u8>> = frames.iter().map(luma).collect();

    for mbps in [2u64, 6, 15] {
        let mut enc = encoder_for(CodecFormat::H264);
        enc.set_target_bitrate((mbps * 1_000_000) as usize);
        enc.set_fps(15);
        let mut dec = NativeDecoder::new();
        let mut sizes = Vec::new();
        for f in &frames {
            let e = enc.encode(f).expect("encode");
            if e.data.is_empty() {
                continue;
            }
            sizes.push(e.data.len());
            if let Ok(fr) = dec.decode(&e) {
                // Latencia/lookahead del MFT: empareja contra la fuente MÁS
                // parecida (no asumimos orden exacto de salida).
                let cur = luma(&fr);
                let mut best = (0.0f64, 0usize);
                for (i, src_y) in luma_src.iter().enumerate() {
                    let p = psnr(src_y, &cur);
                    if p > best.0 {
                        best = (p, i);
                    }
                }
                eprintln!(
                    "  {mbps}Mbps frame {} -> {:.1} KB · PSNR {:.1} dB (fuente #{})",
                    sizes.len(),
                    e.data.len() as f64 / 1024.0,
                    best.0,
                    best.1
                );
            }
        }
        let total: usize = sizes.iter().sum();
        eprintln!(
            "objetivo={mbps} Mbps · emitidos={} · total={} ({:.1} KB/frame)",
            sizes.len(),
            total,
            total as f64 / sizes.len().max(1) as f64 / 1024.0
        );
    }
}
