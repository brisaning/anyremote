//! Backend Windows de portapapeles (CF_UNICODETEXT vía user32 + kernel32).
//! Patrón H-RD-06/07: plataforma propia, implementación clean-room (D-01).
//! La escritura es efecto local real: solo debe invocarse bajo permiso de sesión (D-07).

use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;

use crate::{ClipboardBackend, ClipboardError};

fn with_clipboard<T>(f: impl FnOnce() -> Result<T, ClipboardError>) -> Result<T, ClipboardError> {
    unsafe { OpenClipboard(None) }
        .map_err(|e| ClipboardError::Platform(format!("OpenClipboard: {e}")))?;
    let out = f();
    let _ = unsafe { CloseClipboard() };
    out
}

pub struct WinClipboard;

impl WinClipboard {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WinClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardBackend for WinClipboard {
    fn read_text(&mut self) -> Result<Option<String>, ClipboardError> {
        with_clipboard(|| {
            if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32) }.is_err() {
                return Ok(None); // sin texto (imágenes, html, vacío...)
            }
            let handle = unsafe { GetClipboardData(CF_UNICODETEXT.0 as u32) }
                .map_err(|e| ClipboardError::Platform(format!("GetClipboardData: {e}")))?;
            if handle.is_invalid() {
                return Err(ClipboardError::Unavailable("handle nulo".into()));
            }
            let global = HGLOBAL(handle.0);
            let ptr = unsafe { GlobalLock(global) };
            if ptr.is_null() {
                return Err(ClipboardError::Platform("GlobalLock nulo".into()));
            }
            let size = unsafe { GlobalSize(global) };
            let wide: Vec<u16> = unsafe {
                let slice = std::slice::from_raw_parts(ptr as *const u16, size / 2);
                let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
                slice[..end].to_vec()
            };
            unsafe { GlobalUnlock(global) };
            Ok(Some(String::from_utf16_lossy(&wide)))
        })
    }

    fn write_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        wide.push(0);
        let bytes = wide.len() * 2;
        with_clipboard(|| {
            unsafe { EmptyClipboard() }
                .map_err(|e| ClipboardError::Platform(format!("EmptyClipboard: {e}")))?;
            let mem = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) }
                .map_err(|e| ClipboardError::Platform(format!("GlobalAlloc: {e}")))?;
            let ptr = unsafe { GlobalLock(mem) };
            if ptr.is_null() {
                return Err(ClipboardError::Platform("GlobalLock nulo".into()));
            }
            unsafe {
                std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr as *mut u8, bytes);
                GlobalUnlock(mem);
            }
            // SetClipboardData asume la propiedad de `mem` en éxito.
            unsafe { SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(mem.0))) }.map_err(|e| {
                ClipboardError::Platform(format!("SetClipboardData: {e}"))
            })?;
            Ok(())
        })
    }

    fn description(&self) -> String {
        "windows-clipboard-cf-unicodetext".into()
    }
}

/// Escritura SOLO bajo opt-in explícito (evita pisar el portapapeles en tests automáticos).
/// Activar con `ANYREMOTE_TEST_CLIPBOARD=1` (prueba manual/CI dedicada).
#[test]
fn windows_clipboard_read_probe() {
    let mut c = WinClipboard::new();
    // Lectura segura: no modifica el contenido actual.
    match c.read_text() {
        Ok(_) => {}
        Err(ClipboardError::Platform(p)) => panic!("backend roto: {p}"),
        Err(e) => panic!("error inesperado: {e:?}"),
    }
    assert_eq!(c.description(), "windows-clipboard-cf-unicodetext");
}

#[test]
#[cfg_attr(not(any(test)), allow(dead_code))]
fn windows_clipboard_roundtrip_only_when_opted_in() {
    if std::env::var("ANYREMOTE_TEST_CLIPBOARD").ok().as_deref() != Some("1") {
        eprintln!("omitido: define ANYREMOTE_TEST_CLIPBOARD=1 para el roundtrip real");
        return;
    }
    let mut c = WinClipboard::new();
    let original = c.read_text().unwrap();
    c.write_text("anyremote-roundtrip").unwrap();
    assert_eq!(c.read_text().unwrap().as_deref(), Some("anyremote-roundtrip"));
    if let Some(orig) = original {
        let _ = c.write_text(&orig); // restaurar
    }
}
