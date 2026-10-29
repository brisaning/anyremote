//! Cursor del sistema para el **overlay del visor** (C-11, clean-room D-01).
//!
//! DXGI **no** dibuja el cursor hardware en los frames capturados: sin este
//! módulo el controlador no ve dónde está el cursor remoto. El anfitrión lo
//! sondea (~50 ms) y publica **solo cambios** por un canal `watch`.
//!
//! Coordenadas: se normalizan a **0..65535 contra el monitor primario**, la
//! misma métrica que el inyector usa con `MOUSEEVENTF_ABSOLUTE` (win32) y que
//! el visor usa al mapear su caja → **el overlay aparece donde caen los
//! clics** (espejo de `normalizedPointer` en la app).

/// Estado normalizado del cursor (0..65535, monitor primario).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CursorPos {
    pub nx: u32,
    pub ny: u32,
    pub visible: bool,
}

/// Normaliza coordenadas de pantalla a 0..65535 (espejo del mapeo inverso
/// que hace el visor: `pos/size * 65535` con **denominador = tamaño**, igual
/// que `normalizedPointer` en Dart). Fuera de rango → se recorta al borde
/// (monitores secundarios: coincide con el comportamiento del inyector).
pub fn normalize_cursor(x: i32, y: i32, w: i32, h: i32) -> (u32, u32) {
    fn axis(v: i32, size: i32) -> u32 {
        if size <= 1 {
            return 0;
        }
        let v = v.clamp(0, size - 1) as u64;
        ((v * 65535) / size as u64) as u32
    }
    (axis(x, w), axis(y, h))
}

/// Fuente del estado del cursor.
#[derive(Clone, Debug, Default)]
pub enum CursorBackend {
    /// Producción: `GetCursorPos` + `GetCursorInfo` + métricas del primario.
    #[default]
    Windows,
    /// Tests: posición fija ya normalizada (sin cambios → **un solo** evento).
    Fixed(CursorPos),
    /// Tests: avanza en cada consulta (varios eventos por sesión).
    Sequence { points: Vec<CursorPos>, idx: usize },
}

impl CursorBackend {
    /// Consulta el estado actual (`None` = sin dato, se reintenta al rato).
    pub fn poll(&mut self) -> Option<CursorPos> {
        match self {
            Self::Windows => {
                let (x, y, visible) = windows_poll()?;
                let (w, h) = screen_size();
                let (nx, ny) = normalize_cursor(x, y, w, h);
                Some(CursorPos { nx, ny, visible })
            }
            // Ya normalizadas por el test (independientes de la resolución).
            Self::Fixed(p) => Some(*p),
            Self::Sequence { points, idx } => {
                if points.is_empty() {
                    return None;
                }
                let p = points[*idx % points.len()];
                *idx += 1;
                Some(p)
            }
        }
    }
}

/// (x, y, visible) en coordenadas de pantalla del escritorio virtual.
#[cfg(windows)]
fn windows_poll() -> Option<(i32, i32, bool)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorInfo, GetCursorPos, CURSORINFO, CURSOR_SHOWING,
    };
    unsafe {
        let mut pt = POINT::default();
        GetCursorPos(&mut pt).ok()?;
        let mut ci = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        // Sin CURSOR_SHOWING el cursor está oculto (p. ej. remote desktop).
        let visible = GetCursorInfo(&mut ci)
            .map(|_| ci.flags.0 & CURSOR_SHOWING.0 != 0)
            .unwrap_or(true);
        Some((pt.x, pt.y, visible))
    }
}

#[cfg(not(windows))]
fn windows_poll() -> Option<(i32, i32, bool)> {
    None
}

/// Tamaño del monitor primario (la métrica de MOUSEEVENTF_ABSOLUTE).
#[cfg(windows)]
fn screen_size() -> (i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

#[cfg(not(windows))]
fn screen_size() -> (i32, i32) {
    (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_cursor_is_the_inverse_of_the_viewer_mapping() {
        // Espejo de normalizedPointer (Dentro): v/size * 65535.
        assert_eq!(normalize_cursor(0, 0, 1920, 1080), (0, 0));
        assert_eq!(normalize_cursor(960, 540, 1920, 1080), (32767, 32767));
        // Recorte fuera de rango (monitores secundarios / negativos).
        assert_eq!(normalize_cursor(-40, -10, 1920, 1080), (0, 0));
        assert_eq!(
            normalize_cursor(5000, 2000, 1920, 1080),
            normalize_cursor(1919, 1079, 1920, 1080)
        );
        // Monótono y sin desbordes (nunca > 65535).
        let (a, _) = normalize_cursor(100, 0, 1920, 1080);
        let (b, _) = normalize_cursor(200, 0, 1920, 1080);
        assert!(a < b && b <= 65535);
        // Degenerado: sin pantalla no hay posición.
        assert_eq!(normalize_cursor(10, 10, 0, 0), (0, 0));
    }

    #[test]
    fn backends_report_positions_and_sequence_advances() {
        let p = CursorPos {
            nx: 1234,
            ny: 5678,
            visible: true,
        };
        let mut fixed = CursorBackend::Fixed(p);
        assert_eq!(fixed.poll(), Some(p));
        assert_eq!(fixed.poll(), Some(p)); // estable: change-detection arriba

        let mut seq = CursorBackend::Sequence {
            points: vec![p, CursorPos::default()],
            idx: 0,
        };
        assert_eq!(seq.poll(), Some(p));
        assert_eq!(seq.poll(), Some(CursorPos::default()));
        assert_eq!(seq.poll(), Some(p)); // cicla
    }
}
