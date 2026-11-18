//! Sonda C-35 (ignorada en el gate): coste y calidad del encoder con el
//! escritorio real, tras calentar (la creación del MFT no cuenta).
//!
//! Ejecutar: `cargo test --release -p anyremote-daemon --test video_quality_probe
//! -- --ignored --nocapture`

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

fn run(label: &str, w: u32, h: u32, src: &Frame, mbps: u64) {
    let base = scale_frame(src, w, h);
    let frames: Vec<Frame> = (0..30).map(|i| scroll(&base, i * 3)).collect();
    let luma_src: Vec<Vec<u8>> = frames.iter().map(luma).collect();
    let mut enc = encoder_for(CodecFormat::H264);
    enc.set_target_bitrate((mbps * 1_000_000) as usize);
    enc.set_fps(15);
    let mut dec = NativeDecoder::new();
    // Calentamiento (creación del MFT + primeros frames), fuera de la medida.
    for f in frames.iter().take(6) {
        let _ = enc.encode(f);
    }
    let mut sizes = Vec::new();
    let mut best_psnr = 0.0f64;
    let mut total_ms = 0.0f64;
    for f in frames.iter().skip(6) {
        let t = std::time::Instant::now();
        let e = enc.encode(f).expect("encode");
        total_ms += t.elapsed().as_secs_f64() * 1000.0;
        if e.data.is_empty() {
            continue;
        }
        sizes.push(e.data.len());
        if let Ok(fr) = dec.decode(&e) {
            let cur = luma(&fr);
            let p = luma_src.iter().map(|s| psnr(s, &cur)).fold(0.0, f64::max);
            best_psnr = best_psnr.max(p);
        }
    }
    let total: usize = sizes.iter().sum();
    let n = (frames.len() - 6) as f64;
    eprintln!(
        "{label} {w}x{h} @{mbps} Mbps · emitidos={}/{} · {:.1} KB/frame · {:.1} ms/frame · mejor PSNR {best_psnr:.1} dB",
        sizes.len(),
        frames.len() - 6,
        total as f64 / sizes.len().max(1) as f64 / 1024.0,
        total_ms / n
    );
}

#[test]
#[ignore]
fn probe_encoder() {
    let src = initial_snapshot().expect("snapshot del escritorio");
    eprintln!("escritorio: {}x{}", src.width, src.height);
    run("1080p", 1920, 1080, &src, 2);
    run("1080p", 1920, 1080, &src, 6);
    run("1080p", 1920, 1080, &src, 15);
    run("nativa", 3840, 2160, &src, 25);
}
