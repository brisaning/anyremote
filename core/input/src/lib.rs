//! `anyremote-input` — Inyección de teclado/ratón multiplataforma (D-06, patrón H-RD-06:
//! librería de entrada separada del servidor). Windows: `SendInput`.
//! Los eventos solo se inyectan bajo permiso de sesión (D-07, gate C3 usa RecordingInjector).
//! Clean-room D-01.

/// Teclas soportadas en el protocolo de entrada (código = VK de Windows en el backend win32).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    Ctrl,
    Alt,
    Shift,
    Escape,
    Tab,
    Enter,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

/// Botones de ratón.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Evento de entrada normalizado (modelo de protocolo; no depende de plataforma).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum InputEvent {
    KeyDown(Key),
    KeyUp(Key),
    /// Posición **absoluta normalizada 0..65535** de pantalla (ejes X/Y).
    /// El wire no depende de la resolución del vídeo ni de la del anfitrión
    /// (fix campo v0.6.2: antes eran píxeles de pantalla, lo que rompía el
    /// control cuando el vídeo iba escalado — 720p/1080p en pantalla nativa).
    MoveTo { x: u16, y: u16 },
    /// Movimiento relativo (píxeles).
    MoveBy { dx: i32, dy: i32 },
    ButtonDown(MouseButton),
    ButtonUp(MouseButton),
    /// Rueda: positivo = scroll hacia abajo, negativo = arriba.
    Scroll { delta: i32 },
}

#[derive(Debug, PartialEq)]
pub enum InputError {
    Unsupported(String),
    Platform(String),
    Denied,
}

/// Inyector de eventos de entrada.
pub trait InputInjector {
    fn inject(&mut self, event: &InputEvent) -> Result<(), InputError>;
    fn description(&self) -> String;
}

/// Inyector que solo registra (tests y gate C3 — no toca el sistema).
#[derive(Default)]
pub struct RecordingInjector {
    pub events: Vec<InputEvent>,
}

impl RecordingInjector {
    pub fn new() -> Self {
        Self::default()
    }
}

impl InputInjector for RecordingInjector {
    fn inject(&mut self, event: &InputEvent) -> Result<(), InputError> {
        self.events.push(*event);
        Ok(())
    }

    fn description(&self) -> String {
        format!("recording({} eventos)", self.events.len())
    }
}

/// Enum unificador (patrón H-RD-05/06 por plataforma).
pub enum PlatformInjector {
    Recording(RecordingInjector),
    #[cfg(windows)]
    Windows(windows_backend::SendInputInjector),
}

impl InputInjector for PlatformInjector {
    fn inject(&mut self, event: &InputEvent) -> Result<(), InputError> {
        match self {
            Self::Recording(i) => i.inject(event),
            #[cfg(windows)]
            Self::Windows(i) => i.inject(event),
        }
    }

    fn description(&self) -> String {
        match self {
            Self::Recording(i) => i.description(),
            #[cfg(windows)]
            Self::Windows(i) => i.description(),
        }
    }
}

#[cfg(windows)]
pub mod windows_backend;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_injector_captures_sequence() {
        let mut rec = RecordingInjector::new();
        rec.inject(&InputEvent::KeyDown(Key::Char('a'))).unwrap();
        rec.inject(&InputEvent::MoveTo { x: 100, y: 200 }).unwrap();
        rec.inject(&InputEvent::ButtonUp(MouseButton::Left)).unwrap();
        assert_eq!(
            rec.events,
            vec![
                InputEvent::KeyDown(Key::Char('a')),
                InputEvent::MoveTo { x: 100, y: 200 },
                InputEvent::ButtonUp(MouseButton::Left),
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_backend_describes_itself_without_injecting() {
        // El backend real solo se construye; la inyección requiere permiso de sesión
        // y nunca se ejercita en tests automáticos (movería el ratón real).
        let inj = windows_backend::SendInputInjector::new();
        assert!(inj.description().contains("sendinput"));
    }
}
