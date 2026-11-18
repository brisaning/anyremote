//! Sonda C-36 (ignorada): coste del escalado 4K→1080p/720p con el scaler
//! paralelo, y comprobación de que el resultado es byte a byte el mismo que el
//! de la versión serie.
//!
//! Ejecutar: `cargo test --release -p anyremote-daemon --test scale_probe --
//! --ignored --nocapture`

use anyremote_capture::{Frame, PixelFormat, TestPatternCapturer};

/// Implementación serie de referencia (la de C-31, sin hilos ni caché).
fn reference(src: &Frame, w: u32, h: u32) -> Frame {
    if src.width == w && src.height == h {
        return src.clone();
    }
    let (sw, sh) = (src.width as usize, src.height as usize);
    let (dw, dh) = (w as usize, h as usize);
    let table = |n: usize, m: usize| -> Vec<(usize, usize, u32)> {
        (0..m)
            .map(|i| {
                let f = (i as f64 + 0.5) * n as f64 / m as f64 - 0.5;
                let i0 = f.floor().max(0.0) as usize;
                let i1 = (i0 + 1).min(n - 1);
                let w = ((f - i0 as f64).clamp(0.0, 1.0) * 65536.0).round() as u32;
                (i0, i1, w)
            })
            .collect()
    };
    let xtab = table(sw, dw);
    let ytab = table(sh, dh);
    let mut scratch = vec![0u8; dw * sh * 4];
    for y in 0..sh {
        let row = &src.data[y * sw * 4..(y + 1) * sw * 4];
        let out_row = &mut scratch[y * dw * 4..(y + 1) * dw * 4];
        for (x, &(x0, x1, wx)) in xtab.iter().enumerate() {
            let (i0, i1, o) = (x0 * 4, x1 * 4, x * 4);
            for c in 0..3 {
                let a = row[i0 + c] as u32;
                let b = row[i1 + c] as u32;
                out_row[o + c] = ((a * (65536 - wx) + b * wx) >> 16).min(255) as u8;
            }
            out_row[o + 3] = 255;
        }
    }
    let mut data = vec![0u8; dw * dh * 4];
    for (y, &(y0, y1, wy)) in ytab.iter().enumerate() {
        let r0 = &scratch[y0 * dw * 4..(y0 + 1) * dw * 4];
        let r1 = &scratch[y1 * dw * 4..(y1 + 1) * dw * 4];
        let o = y * dw * 4;
        for x in 0..dw {
            let i = x * 4;
            for c in 0..3 {
                let a = r0[i + c] as u32;
                let b = r1[i + c] as u32;
                data[o + i + c] = ((a * (65536 - wy) + b * wy) >> 16).min(255) as u8;
            }
            data[o + i + 3] = 255;
        }
    }
    Frame {
        width: w,
        height: h,
        format: PixelFormat::Bgra8,
        data,
    }
}

/// C-36: el escaler paralelo debe dar EXACTAMENTE lo mismo que el serie.
#[test]
fn parallel_scale_matches_reference() {
    let src = TestPatternCapturer::render(640, 360, 7);
    let mut scaler = anyremote_daemon::video::FrameScaler::new();
    for (w, h) in [(320u32, 180u32), (427, 240), (640, 360), (160, 90)] {
        let got = scaler.scale(&src, w, h);
        let want = reference(&src, w, h);
        assert_eq!(
            (got.width, got.height, got.data.len()),
            (want.width, want.height, want.data.len())
        );
        assert_eq!(got.data, want.data, "dims {w}x{h}");
    }
}

#[test]
#[ignore]
fn probe_scale_cost() {
    let src = TestPatternCapturer::render(3840, 2160, 7);
    for (w, h) in [(1920u32, 1080u32), (1280, 720), (1632, 918)] {
        let mut scaler = anyremote_daemon::video::FrameScaler::new();
        // Calentamiento + equivalencia.
        let got = scaler.scale(&src, w, h);
        let want = reference(&src, w, h);
        assert_eq!(got.data, want.data, "el escaler paralelo debe ser idéntico");
        let n = 20;
        let t = std::time::Instant::now();
        for _ in 0..n {
            let _ = scaler.scale(&src, w, h);
        }
        let par = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        let t = std::time::Instant::now();
        for _ in 0..n {
            let _ = reference(&src, w, h);
        }
        let ser = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        eprintln!(
            "3840x2160 -> {w}x{h}: serie {ser:.1} ms · paralelo {par:.1} ms · {:.1}x",
            ser / par
        );
    }
}
