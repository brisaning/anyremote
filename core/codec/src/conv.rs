//! Conversiones de color del pipeline (C-14): BGRA8 ↔ I420 en filas compactas.
//! Extraídas de `vpx_backend` para que también las use el backend H.264/MF
//! sin depender de la feature `vpx`. Rango completo en ambos sentidos
//! (par coherente, como en su origen).

pub fn bgra_to_i420(width: u32, height: u32, bgra: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let w = width as usize;
    let h = height as usize;
    debug_assert!(
        w.is_multiple_of(2) && h.is_multiple_of(2),
        "dimensiones pares requeridas (4:2:0)"
    );
    let mut y = vec![0u8; w * h];
    let cw = w / 2;
    let mut u = vec![0u8; cw * (h / 2)];
    let mut v = vec![0u8; cw * (h / 2)];

    for row in 0..h {
        for col in 0..w {
            let i = (row * w + col) * 4;
            let b = bgra[i] as i32;
            let g = bgra[i + 1] as i32;
            let r = bgra[i + 2] as i32;
            let yy = (77 * r + 150 * g + 29 * b) >> 8; // rango completo sin offset
            y[row * w + col] = yy.clamp(0, 255) as u8;
            if row % 2 == 0 && col % 2 == 0 {
                let mut sb = 0i32;
                let mut sg = 0i32;
                let mut sr = 0i32;
                for dy in 0..2usize {
                    for dx in 0..2usize {
                        let j = ((row + dy) * w + (col + dx)) * 4;
                        sb += bgra[j] as i32;
                        sg += bgra[j + 1] as i32;
                        sr += bgra[j + 2] as i32;
                    }
                }
                let (b, g, r) = (sb / 4, sg / 4, sr / 4);
                let uu = ((-43 * r - 85 * g + 128 * b + 128) >> 8) + 128;
                let vv = ((128 * r - 107 * g - 21 * b + 128) >> 8) + 128;
                let ci = (row / 2) * cw + col / 2;
                u[ci] = uu.clamp(0, 255) as u8;
                v[ci] = vv.clamp(0, 255) as u8;
            }
        }
    }
    (y, u, v)
}

/// BGRA8 → NV12 **en una pasada** (C-35): Y por píxel y UV por bloque 2×2,
/// escribiendo directamente en `out` (sin vectores intermedios ni allocs por
/// frame). Medido: la cadena anterior (BGRA→I420 + I420→NV12) costaba 16 ms a
/// 1080p y 52 ms a 4K — más que el propio encoder. Con una pasada: 7,6/29,8 ms;
/// por bandas en paralelo baja otra vez (ver [`bgra_to_nv12_into`]).
pub fn bgra_to_nv12_into(width: u32, height: u32, bgra: &[u8], out: &mut [u8]) {
    let w = width as usize;
    let h = height as usize;
    debug_assert!(w.is_multiple_of(2) && h.is_multiple_of(2));
    let y_len = w * h;
    debug_assert!(out.len() >= y_len + y_len / 2);
    debug_assert!(bgra.len() >= w * h * 4);

    let (y_plane, uv_plane) = out.split_at_mut(y_len);
    let pairs = h / 2;
    // Hilos: solo compensa en frames grandes (el spawn cuesta ~0,1 ms).
    let hilos = if h >= 720 {
        std::thread::available_parallelism()
            .map(|v| v.get())
            .unwrap_or(1)
            .min(8)
    } else {
        1
    };

    if hilos <= 1 {
        fill_nv12_band(w, bgra, 0, y_plane, uv_plane);
        return;
    }

    // Reparto por filas de PARES: cada banda escribe sus filas Y y su fila UV.
    let mut jobs = Vec::with_capacity(hilos);
    let mut y_rest = y_plane;
    let mut uv_rest = uv_plane;
    for k in 0..hilos {
        let p0 = pairs * k / hilos;
        let p1 = pairs * (k + 1) / hilos;
        let y_bytes = (p1 - p0) * 2 * w;
        let uv_bytes = (p1 - p0) * w;
        let (yb, y_next) = y_rest.split_at_mut(y_bytes);
        let (ub, u_next) = uv_rest.split_at_mut(uv_bytes);
        y_rest = y_next;
        uv_rest = u_next;
        jobs.push((p0, yb, ub));
    }
    std::thread::scope(|scope| {
        for (p0, yb, ub) in jobs {
            scope.spawn(move || fill_nv12_band(w, bgra, p0, yb, ub));
        }
    });
}

/// Rellena una banda de NV12: `pair0` es la primera fila de PARES (2 px).
fn fill_nv12_band(w: usize, bgra: &[u8], pair0: usize, y: &mut [u8], uv: &mut [u8]) {
    let rows = y.len() / w;
    for r in 0..rows {
        let row = pair0 * 2 + r;
        let src = row * w * 4;
        let dst = r * w;
        for col in 0..w {
            let i = src + col * 4;
            let yy =
                (77 * bgra[i + 2] as i32 + 150 * bgra[i + 1] as i32 + 29 * bgra[i] as i32) >> 8;
            y[dst + col] = yy.clamp(0, 255) as u8;
        }
    }
    // UV entrelazado: una fila por cada par de filas Y.
    for p in 0..rows / 2 {
        let row = (pair0 + p) * 2;
        let uv_row = p * w;
        for col in (0..w).step_by(2) {
            let i = row * w * 4 + col * 4;
            let j = i + w * 4;
            let b =
                (bgra[i] as i32 + bgra[i + 4] as i32 + bgra[j] as i32 + bgra[j + 4] as i32) >> 2;
            let g =
                (bgra[i + 1] as i32 + bgra[i + 5] as i32 + bgra[j + 1] as i32 + bgra[j + 5] as i32)
                    >> 2;
            let r =
                (bgra[i + 2] as i32 + bgra[i + 6] as i32 + bgra[j + 2] as i32 + bgra[j + 6] as i32)
                    >> 2;
            uv[uv_row + col] =
                (((-43 * r - 85 * g + 128 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
            uv[uv_row + col + 1] =
                (((128 * r - 107 * g - 21 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
        }
    }
}

/// I420 → BGRA8 (BT.601; croma por muestreo simple).
pub fn i420_to_bgra(
    width: usize,
    height: usize,
    y: &[u8],
    u: &[u8],
    v: &[u8],
    y_s: usize,
    c_s: usize,
) -> Vec<u8> {
    let mut out = vec![0u8; width * height * 4];
    for row in 0..height {
        for col in 0..width {
            let yy = y[row * y_s + col] as i32; // rango completo (par coherente con bgra_to_i420)
            let uu = u[(row / 2) * c_s + col / 2] as i32;
            let vv = v[(row / 2) * c_s + col / 2] as i32;
            let ep = vv - 128;
            let eu = uu - 128;
            let r = (256 * yy + 359 * ep + 128) >> 8;
            let g = (256 * yy - 88 * eu - 183 * ep + 128) >> 8;
            let b = (256 * yy + 453 * eu + 128) >> 8;
            let i = (row * width + col) * 4;
            out[i] = b.clamp(0, 255) as u8;
            out[i + 1] = g.clamp(0, 255) as u8;
            out[i + 2] = r.clamp(0, 255) as u8;
            out[i + 3] = 255;
        }
    }
    out
}

/// Recorta BGRA a `ew`×`eh` (dims pares para NV12) manteniendo la fila 0,0.
pub fn crop_bgra(src: &[u8], stride_w: u32, ew: u32, eh: u32) -> Vec<u8> {
    let src_stride = stride_w as usize * 4;
    let dst_stride = ew as usize * 4;
    let mut out = vec![0u8; dst_stride * eh as usize];
    for row in 0..eh as usize {
        let from = row * src_stride;
        let to = row * dst_stride;
        out[to..to + dst_stride].copy_from_slice(&src[from..from + dst_stride]);
    }
    out
}

/// I420 → NV12 (croma entrelazado UV).
pub fn i420_to_nv12(y: &[u8], u: &[u8], v: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(y.len() + u.len() * 2);
    out.extend_from_slice(y);
    for (a, b) in u.iter().zip(v.iter()) {
        out.push(*a);
        out.push(*b);
    }
    out
}

/// NV12 → I420 (separa el croma entrelazado).
pub fn nv12_to_i420(w: u32, h: u32, nv12: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let y_len = (w * h) as usize;
    let y = nv12[..y_len].to_vec();
    let uv = &nv12[y_len..];
    let pairs = (w as usize / 2) * (h as usize / 2);
    let mut u = Vec::with_capacity(pairs);
    let mut v = Vec::with_capacity(pairs);
    for i in 0..pairs {
        u.push(uv[i * 2]);
        v.push(uv[i * 2 + 1]);
    }
    (y, u, v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nv12_roundtrip_is_lossless() {
        let (w, h) = (16u32, 8u32);
        let y: Vec<u8> = (0..w * h).map(|i| (i % 251) as u8).collect();
        let pairs = (w * h / 4) as usize;
        let u: Vec<u8> = (0..pairs).map(|i| (i * 3 % 253) as u8).collect();
        let v: Vec<u8> = (0..pairs).map(|i| (i * 7 % 249) as u8).collect();
        let nv12 = i420_to_nv12(&y, &u, &v);
        assert_eq!(nv12.len(), (w * h) as usize + pairs * 2);
        let (y2, u2, v2) = nv12_to_i420(w, h, &nv12);
        assert_eq!(y, y2);
        assert_eq!(u, u2);
        assert_eq!(v, v2);
    }
}
