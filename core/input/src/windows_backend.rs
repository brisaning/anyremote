//! Backend Windows `SendInput` (patrón H-RD-06: inyección multiplataforma).
//! La inyección es un efecto secundario real: solo debe llamarse con permiso de
//! sesión activo (D-07). Clean-room D-01.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS,
    VIRTUAL_KEY, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_MENU,
    VK_RETURN, VK_RIGHT, VK_SHIFT, VK_TAB, VK_UP, VkKeyScanW,
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

/// (vk, requiere_shift) para una tecla del protocolo.
fn vk_for(key: Key) -> Result<(u16, bool), InputError> {
    let (vk, shift) = match key {
        Key::Char(c) => {
            let packed = unsafe { VkKeyScanW(c as u16) };
            if packed == -1 {
                return Err(InputError::Unsupported(format!("tecla '{c}' sin VK")));
            }
            ((packed as u16) & 0xff, ((packed as u16) & 0x100) != 0)
        }
        Key::Ctrl => (VK_CONTROL.0 as u16, false),
        Key::Alt => (VK_MENU.0 as u16, false),
        Key::Shift => (VK_SHIFT.0 as u16, false),
        Key::Escape => (VK_ESCAPE.0 as u16, false),
        Key::Tab => (VK_TAB.0 as u16, false),
        Key::Enter => (VK_RETURN.0 as u16, false),
        Key::Backspace => (0x08, false),
        Key::Delete => (VK_DELETE.0 as u16, false),
        Key::Left => (VK_LEFT.0 as u16, true),
        Key::Right => (VK_RIGHT.0 as u16, true),
        Key::Up => (VK_UP.0 as u16, true),
        Key::Down => (VK_DOWN.0 as u16, true),
        Key::Home => (VK_HOME.0 as u16, true),
        Key::End => (VK_END.0 as u16, true),
    };
    Ok((vk, shift))
}

fn key_input(vk: u16, up: bool, extended: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if up {
        flags = flags | KEYEVENTF_KEYUP;
    }
    if extended {
        flags = flags | KEYEVENTF_EXTENDEDKEY;
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
        let (vk, shift) = vk_for(key)?;
        let extended = matches!(
            key,
            Key::Left | Key::Right | Key::Up | Key::Down | Key::Home | Key::End | Key::Delete
        );
        let mut inputs = Vec::with_capacity(2);
        if shift {
            inputs.push(key_input(VK_SHIFT.0 as u16, up, false));
        }
        inputs.push(key_input(vk, up, extended));
        Self::send(&inputs)
    }
}
