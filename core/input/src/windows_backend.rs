//! Backend Windows `SendInput` (patrón H-RD-06: inyección multiplataforma).
//! La inyección es un efecto secundario real: solo debe llamarse con permiso de
//! sesión activo (D-07). Clean-room D-01.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, VkKeyScanW, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY, VK_CAPITAL, VK_CONTROL,
    VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F10, VK_F11, VK_F12, VK_F2, VK_F3, VK_F4,
    VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_HOME, VK_INSERT, VK_LEFT, VK_MENU, VK_NEXT, VK_NUMLOCK,
    VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SCROLL, VK_SHIFT, VK_TAB, VK_UP,
};

use crate::{InputError, InputEvent, InputInjector, Key, MouseButton};

const WHEEL_DELTA: i32 = 120;

pub struct SendInputInjector;

impl SendInputInjector {
    pub fn new() -> Self {
        Self
    }

    fn send(inputs: &[INPUT]) -> Result<(), InputError> {
        let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            Err(InputError::Platform(format!(
                "SendInput envió {sent}/{}",
                inputs.len()
            )))
        } else {
            Ok(())
        }
    }
}

impl Default for SendInputInjector {
    fn default() -> Self {
        Self::new()
    }
}

/// C-37: cómo inyectar una tecla lógica.
enum Target {
    /// Tecla física (VK) + si necesita Shift (la resuelve el teclado remoto).
    Vk(u16, bool),
    /// Carácter exacto por `KEYEVENTF_UNICODE` (acentos, ñ, €…): no depende del
    /// layout del anfitrión y evita el bug de la dead key (antes se inyectaba
    /// `´` y luego la vocal → `´o`).
    Text(u16),
}

/// Convierte una tecla del protocolo en la inyección concreta.
fn target_for(key: Key) -> Result<Target, InputError> {
    let (vk, shift) = match key {
        Key::Char(c) => {
            // ASCII: VK (conserva atajos Ctrl+C, Ctrl+V…). No ASCII: Unicode.
            if !c.is_ascii() {
                return Ok(Target::Text(c as u32 as u16));
            }
            let packed = unsafe { VkKeyScanW(c as u16) };
            if packed == -1 {
                return Ok(Target::Text(c as u16));
            }
            ((packed as u16) & 0xff, ((packed as u16) & 0x100) != 0)
        }
        Key::Ctrl => (VK_CONTROL.0, false),
        Key::Alt => (VK_MENU.0, false),
        Key::Shift => (VK_SHIFT.0, false),
        Key::Escape => (VK_ESCAPE.0, false),
        Key::Tab => (VK_TAB.0, false),
        Key::Enter => (VK_RETURN.0, false),
        Key::Backspace => (0x08, false),
        Key::Delete => (VK_DELETE.0, false),
        // C-38: el segundo campo es "necesita Shift". Las teclas de
        // navegación lo llevaban en `true` → cada flecha/RePág/AvPág se
        // inyectaba con Shift y el anfitrión SELECCIONABA texto.
        Key::Left => (VK_LEFT.0, false),
        Key::Right => (VK_RIGHT.0, false),
        Key::Up => (VK_UP.0, false),
        Key::Down => (VK_DOWN.0, false),
        Key::Home => (VK_HOME.0, false),
        Key::End => (VK_END.0, false),
        Key::Insert => (VK_INSERT.0, false),
        Key::PageUp => (VK_PRIOR.0, false),
        Key::PageDown => (VK_NEXT.0, false),
        Key::CapsLock => (VK_CAPITAL.0, false),
        Key::NumLock => (VK_NUMLOCK.0, false),
        Key::ScrollLock => (VK_SCROLL.0, false),
        Key::F1 => (VK_F1.0, false),
        Key::F2 => (VK_F2.0, false),
        Key::F3 => (VK_F3.0, false),
        Key::F4 => (VK_F4.0, false),
        Key::F5 => (VK_F5.0, false),
        Key::F6 => (VK_F6.0, false),
        Key::F7 => (VK_F7.0, false),
        Key::F8 => (VK_F8.0, false),
        Key::F9 => (VK_F9.0, false),
        Key::F10 => (VK_F10.0, false),
        Key::F11 => (VK_F11.0, false),
        Key::F12 => (VK_F12.0, false),
    };
    Ok(Target::Vk(vk, shift))
}

/// Entrada de teclado por carácter Unicode (sin VK, sin layout).
fn text_input(unit: u16, up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn key_input(vk: u16, up: bool, extended: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn mouse_input(dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

impl InputInjector for SendInputInjector {
    fn inject(&mut self, event: &InputEvent) -> Result<(), InputError> {
        match event {
            InputEvent::KeyDown(k) => self.send_key(*k, false),
            InputEvent::KeyUp(k) => self.send_key(*k, true),
            InputEvent::MoveTo { x, y } => {
                // Coordenadas YA normalizadas 0..65535 (modelo de dominio):
                // el valor viaja sin depender de la resolución del vídeo ni de
                // la pantalla, y Windows lo entiende directo con ABSOLUTE.
                Self::send(&[mouse_input(
                    *x as i32,
                    *y as i32,
                    0,
                    MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                )])
            }
            InputEvent::MoveBy { dx, dy } => {
                Self::send(&[mouse_input(*dx, *dy, 0, MOUSEEVENTF_MOVE)])
            }
            InputEvent::ButtonDown(b) => Self::send(&[mouse_input(0, 0, 0, btn_flag(*b, false))]),
            InputEvent::ButtonUp(b) => Self::send(&[mouse_input(0, 0, 0, btn_flag(*b, true))]),
            InputEvent::Scroll { delta } => Self::send(&[mouse_input(
                0,
                0,
                (delta * WHEEL_DELTA) as u32,
                MOUSEEVENTF_WHEEL,
            )]),
        }
    }

    fn description(&self) -> String {
        "windows-sendinput".into()
    }
}

fn btn_flag(b: MouseButton, up: bool) -> MOUSE_EVENT_FLAGS {
    match (b, up) {
        (MouseButton::Left, false) => MOUSEEVENTF_LEFTDOWN,
        (MouseButton::Left, true) => MOUSEEVENTF_LEFTUP,
        (MouseButton::Right, false) => MOUSEEVENTF_RIGHTDOWN,
        (MouseButton::Right, true) => MOUSEEVENTF_RIGHTUP,
        (MouseButton::Middle, false) => MOUSEEVENTF_MIDDLEDOWN,
        (MouseButton::Middle, true) => MOUSEEVENTF_MIDDLEUP,
    }
}

impl SendInputInjector {
    fn send_key(&self, key: Key, up: bool) -> Result<(), InputError> {
        match target_for(key)? {
            Target::Text(unit) => Self::send(&[text_input(unit, up)]),
            Target::Vk(vk, shift) => {
                let extended = matches!(
                    key,
                    Key::Left
                        | Key::Right
                        | Key::Up
                        | Key::Down
                        | Key::Home
                        | Key::End
                        | Key::Delete
                        | Key::Insert
                        | Key::PageUp
                        | Key::PageDown
                        | Key::NumLock
                );
                let mut inputs = Vec::with_capacity(2);
                if shift {
                    inputs.push(key_input(VK_SHIFT.0, up, false));
                }
                inputs.push(key_input(vk, up, extended));
                Self::send(&inputs)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C-37: los caracteres no ASCII van por `KEYEVENTF_UNICODE` (independiente
    /// del layout del anfitrión); el bug de acentos venía de inyectar la dead
    /// key (`´`) y luego la vocal.
    #[test]
    fn non_ascii_chars_use_unicode() {
        assert!(matches!(target_for(Key::Char('á')), Ok(Target::Text(0xE1))));
        assert!(matches!(target_for(Key::Char('ñ')), Ok(Target::Text(0xF1))));
        assert!(matches!(target_for(Key::Char('ü')), Ok(Target::Text(0xFC))));
        assert!(matches!(
            target_for(Key::Char('€')),
            Ok(Target::Text(0x20AC))
        ));
    }

    /// ASCII conserva VK (los atajos Ctrl+C / Ctrl+V siguen siendo atajos).
    #[test]
    fn ascii_chars_keep_the_virtual_key() {
        assert!(matches!(
            target_for(Key::Char('a')),
            Ok(Target::Vk(0x41, false))
        ));
        assert!(matches!(
            target_for(Key::Char('A')),
            Ok(Target::Vk(0x41, true))
        ));
    }

    /// C-37: teclas de edición y bloqueos nuevas.
    #[test]
    fn editing_and_lock_keys_have_vk_without_shift() {
        for (key, vk) in [
            (Key::Insert, 0x2D),
            (Key::PageUp, 0x21),
            (Key::PageDown, 0x22),
            (Key::CapsLock, 0x14),
            (Key::NumLock, 0x90),
            (Key::ScrollLock, 0x91),
            (Key::Left, 0x25),
            (Key::Right, 0x27),
            (Key::Up, 0x26),
            (Key::Down, 0x28),
            (Key::Home, 0x24),
            (Key::End, 0x23),
            (Key::Delete, 0x2E),
        ] {
            assert!(
                matches!(target_for(key), Ok(Target::Vk(v, false)) if v == vk),
                "{key:?} debe ir sin Shift (vk {vk:#x})"
            );
        }
    }
}
