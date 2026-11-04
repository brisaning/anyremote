//! C-23: captura inicial por **GDI** — DXGI Desktop Duplication no entrega
//! nada hasta que el escritorio cambia (QW-1), así que un arranque en frío
//! con pantalla estática no producía NI UN frame: el visor se quedaba en
//! «esperando primer frame» y, como su input solo se monta con imagen, el
//! usuario no podía ni despertar la captura (deadlock: sin frame → sin
//! input → sin cambios → sin frame).
//!
//! [`gdi_snapshot`](crate::initial_snapshot) da la imagen INICIAL (BitBlt
//! del escritorio primario); a partir de ahí el bucle de vídeo reemite el
//! último frame a 1 Hz (pantalla estática) y los cambios reales retoman el
//! flujo DXGI normal.
//!
//! **DPI**: el proceso arranca sin awareness y GDI devolvería el escritorio
//! virtualizado (a escala — y en modo nativa fijaría mal la resolución). La
//! primera llamada fija Per-Monitor V2. Es seguro: la inyección de ratón usa
//! coordenadas normalizadas (`MOUSEEVENTF_ABSOLUTE`) y DXGI ya trabaja en
//! píxeles físicos.
//!
//! Clean-room D-01: implementación propia sobre la API pública de Win32.

use std::sync::Once;

use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
    SRCCOPY,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

use crate::{CaptureError, Frame, PixelFormat};

static DPI_AWARE: Once = Once::new();

/// Fija (una vez por proceso) la awareness Per-Monitor V2 — best-effort: si
/// algo ya la fijó, el API falla y seguimos con la existente.
fn ensure_dpi_aware() {
    DPI_AWARE.call_once(|| unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    });
}

/// Snapshot del escritorio primario en BGRA8 (filas compactas) — ver módulo.
pub fn gdi_snapshot() -> Result<Frame, CaptureError> {
    ensure_dpi_aware();
    unsafe {
        let screen = GetDC(None);
        if screen.0.is_null() {
            return Err(CaptureError::Unavailable(
                "GDI: GetDC falló (sin escritorio)".into(),
            ));
        }
        let w = GetSystemMetrics(SM_CXSCREEN);
        let h = GetSystemMetrics(SM_CYSCREEN);
        if w <= 0 || h <= 0 {
            ReleaseDC(None, screen);
            return Err(CaptureError::Unavailable(
                "GDI: métricas de pantalla inválidas".into(),
            ));
        }
        let (w, h) = (w as u32, h as u32);
        let mem = CreateCompatibleDC(Some(screen));
        if mem.0.is_null() {
            ReleaseDC(None, screen);
            return Err(CaptureError::Unavailable(
                "GDI: CreateCompatibleDC falló".into(),
            ));
        }
        let bmp: HBITMAP = CreateCompatibleBitmap(screen, w as i32, h as i32);
        if bmp.0.is_null() {
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            return Err(CaptureError::Unavailable(
                "GDI: CreateCompatibleBitmap falló".into(),
            ));
        }
        let prev = SelectObject(mem, bmp.into());
        let blit = BitBlt(mem, 0, 0, w as i32, h as i32, Some(screen), 0, 0, SRCCOPY);
        // GetDIBits exige el bitmap NO seleccionado en un DC → restaurar antes.
        SelectObject(mem, prev);
        let mut data = vec![0u8; (w * h * 4) as usize];
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32), // top-down (mismo orden que DXGI)
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            bmiColors: [Default::default()],
        };
        let mut bmi = bmi;
        let got = if blit.is_ok() {
            GetDIBits(
                mem,
                bmp,
                0,
                h,
                Some(data.as_mut_ptr().cast()),
                &mut bmi as *mut BITMAPINFO,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if blit.is_err() {
            return Err(CaptureError::Io("GDI: BitBlt falló".into()));
        }
        if got == 0 {
            return Err(CaptureError::Io("GDI: GetDIBits devolvió 0".into()));
        }
        // BI_RGB deja el4º byte en 0 → el visor (BGRA) lo vería transparente.
        for px in data.as_chunks_mut::<4>().0 {
            px[3] = 0xFF;
        }
        Ok(Frame {
            width: w,
            height: h,
            format: PixelFormat::Bgra8,
            data,
        })
    }
}
