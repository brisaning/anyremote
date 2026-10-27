//! `anyremote-capture` — Captura de pantalla por plataforma (D-06, patrón H-RD-05:
//! `Capturer` por backend con enum unificador). Windows: DXGI Desktop Duplication.
//! Clean-room D-01: sin copia de código AGPL.

/// Formato de píxel canónico del pipeline (BGRA8, como el resto del ecosistema — H-RD-05).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PixelFormat {
    Bgra8,
}

/// Frame de vídeo capturado, filas compactas (stride = width*4).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
}

impl Frame {
    pub fn stride(&self) -> usize {
        self.width as usize * 4
    }
}

#[derive(Debug, PartialEq)]
pub enum CaptureError {
    /// El backend no está disponible (sin escritorio, permisos, entorno headless).
    Unavailable(String),
    Timeout,
    Io(String),
}

/// Capturer: produce el siguiente frame disponible.
pub trait Capturer {
    fn next_frame(&mut self) -> Result<Frame, CaptureError>;
    fn description(&self) -> String;
}

/// Backend sintético determinista (tests, benchmark y fallback sin escritorio).
/// Patrón: gradiente que avanza por frame + contador incrustado en los primeros bytes.
pub struct TestPatternCapturer {
    width: u32,
    height: u32,
    frame_no: u64,
}

impl TestPatternCapturer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            frame_no: 0,
        }
    }

    /// Frame determinista dado un número (para tests de integridad).
    pub fn render(width: u32, height: u32, frame_no: u64) -> Frame {
        let mut data = vec![0u8; (width * height * 4) as usize];
        let shift = (frame_no % 256) as u8;
        for y in 0..height {
            for x in 0..width {
                let i = ((y * width + x) * 4) as usize;
                data[i] = (x as u8).wrapping_add(shift); // B
                data[i + 1] = y as u8; // G
                data[i + 2] = ((x + y) as u8).rotate_left(1); // R
                data[i + 3] = 255; // A
            }
        }
        // Marcador de frame en los primeros 8 bytes (B,G sobrescritos por el contador).
        let n = frame_no.to_le_bytes();
        data[..8].copy_from_slice(&n);
        Frame {
            width,
            height,
            format: PixelFormat::Bgra8,
            data,
        }
    }
}

impl Capturer for TestPatternCapturer {
    fn next_frame(&mut self) -> Result<Frame, CaptureError> {
        let f = Self::render(self.width, self.height, self.frame_no);
        self.frame_no += 1;
        Ok(f)
    }

    fn description(&self) -> String {
        format!("test-pattern {}x{}", self.width, self.height)
    }
}

/// Enum unificador de capturers (patrón H-RD-05: Capturer por SO).
pub enum PlatformCapturer {
    TestPattern(TestPatternCapturer),
    #[cfg(windows)]
    Dxgi(dxgi::DxgiCapturer),
}

impl Capturer for PlatformCapturer {
    fn next_frame(&mut self) -> Result<Frame, CaptureError> {
        match self {
            Self::TestPattern(c) => c.next_frame(),
            #[cfg(windows)]
            Self::Dxgi(c) => c.next_frame(),
        }
    }

    fn description(&self) -> String {
        match self {
            Self::TestPattern(c) => c.description(),
            #[cfg(windows)]
            Self::Dxgi(c) => c.description(),
        }
    }
}

/// Crea el mejor capturer disponible en la plataforma actual.
pub fn best_capturer(width: u32, height: u32) -> PlatformCapturer {
    #[cfg(windows)]
    {
        if let Ok(c) = dxgi::DxgiCapturer::new() {
            return PlatformCapturer::Dxgi(c);
        }
    }
    PlatformCapturer::TestPattern(TestPatternCapturer::new(width, height))
}

#[cfg(windows)]
pub mod dxgi;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_is_deterministic_per_frame() {
        let a = TestPatternCapturer::render(64, 48, 7);
        let b = TestPatternCapturer::render(64, 48, 7);
        let c = TestPatternCapturer::render(64, 48, 8);
        assert_eq!(a, b);
        assert_ne!(a.data, c.data);
        assert_eq!(a.data.len(), 64 * 48 * 4);
        assert_eq!(a.format, PixelFormat::Bgra8);
    }

    #[test]
    fn capturer_increments_frames() {
        let mut c = TestPatternCapturer::new(32, 32);
        let f0 = c.next_frame().unwrap();
        let f1 = c.next_frame().unwrap();
        assert_ne!(f0.data, f1.data);
    }

    #[cfg(windows)]
    #[test]
    fn dxgi_backend_is_available_or_reports_unavailable() {
        // En entornos sin escritorio (servicio, RDP bloqueado) debe reportar
        // Unavailable, no fallar enmascaradamente.
        match dxgi::DxgiCapturer::new() {
            Ok(mut c) => match c.next_frame() {
                Ok(f) => assert!(f.width > 0 && f.height > 0),
                // Pantalla estática desde la apertura de la duplicación: DXGI
                // no entrega nada hasta el primer cambio (comportamiento válido).
                Err(CaptureError::Timeout) => {}
                Err(CaptureError::Unavailable(_)) => {}
                Err(e) => panic!("error inesperado: {e:?}"),
            },
            Err(CaptureError::Unavailable(_)) => {}
            Err(e) => panic!("error inesperado: {e:?}"),
        }
    }
}
