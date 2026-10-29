//! src/engine/input_dispatcher.rs
//! Traducción de Eventos Win32 de Entrada para Galapagos Browser v0.1
//! Conforme a ZNVE v2.4.0 (Axiomas 1 y 2, ARC-01, SEC-01).
//!
//! Responsabilidad: mapeo tipado y determinista de mensajes Win32 (WM_KEYDOWN, WM_KEYUP, WM_CHAR,
//! botones, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_SIZE) a `InputEvent`, que `engine::encode_input_event`
//! serializa hacia el worker. R-03 (INC-20261025-CORE-DEBLOAT): retiradas la conversión al Servo
//! simulado y la cola por pestaña, que se llenaba en cada redimensionado y nunca se vaciaba.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};

/// Constantes estándar de mensajes Win32 de entrada
pub const WM_SIZE: u32 = 0x0005;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_KEYUP: u32 = 0x0101;
pub const WM_CHAR: u32 = 0x0102;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_RBUTTONDOWN: u32 = 0x0204;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_MBUTTONDOWN: u32 = 0x0207;
pub const WM_MBUTTONUP: u32 = 0x0208;
pub const WM_MOUSEWHEEL: u32 = 0x020A;

/// Máscara de modificadores del formato de cable que decodifica `galapagos-content/src/input.rs`
const MOD_SHIFT: u32 = 1 << 0;
const MOD_CTRL: u32 = 1 << 1;
const MOD_ALT: u32 = 1 << 2;
const MOD_META: u32 = 1 << 3;

/// Botones del ratón mapeados de Win32
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Modificadores de teclado
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl KeyModifiers {
    /// Máscara de bits del formato de cable: Shift=1, Ctrl=2, Alt=4, Meta=8
    pub fn to_bitfield(&self) -> u32 {
        let mut bits = 0u32;
        if self.shift {
            bits |= MOD_SHIFT;
        }
        if self.ctrl {
            bits |= MOD_CTRL;
        }
        if self.alt {
            bits |= MOD_ALT;
        }
        if self.meta {
            bits |= MOD_META;
        }
        bits
    }

    /// Captura en caliente los modificadores actuales de Windows
    pub fn query_current_win32() -> Self {
        unsafe {
            let shift = (GetKeyState(VK_SHIFT as i32) as u16 & 0x8000) != 0;
            let ctrl = (GetKeyState(VK_CONTROL as i32) as u16 & 0x8000) != 0;
            let alt = (GetKeyState(VK_MENU as i32) as u16 & 0x8000) != 0;
            Self {
                shift,
                ctrl,
                alt,
                meta: false,
            }
        }
    }
}

/// Evento de entrada de alto nivel tipado y seguro
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputEvent {
    KeyDown {
        key_code: u32,
        modifiers: KeyModifiers,
    },
    KeyUp {
        key_code: u32,
        modifiers: KeyModifiers,
    },
    MouseDown {
        button: MouseButton,
        x: i32,
        y: i32,
        modifiers: KeyModifiers,
    },
    MouseUp {
        button: MouseButton,
        x: i32,
        y: i32,
        modifiers: KeyModifiers,
    },
    MouseMove {
        x: i32,
        y: i32,
        modifiers: KeyModifiers,
    },
    MouseWheel {
        delta_x: f32,
        delta_y: f32,
        x: i32,
        y: i32,
        modifiers: KeyModifiers,
    },
    Resize {
        width: u32,
        height: u32,
    },
    /// DELTA_02: texto ya compuesto por Windows (WM_CHAR). Solo escalares BMP no de control:
    /// los pares sustitutos (emoji) y la composición IME quedan en UX-03.
    Char {
        scalar: char,
    },
}

/// Traductor sin estado de mensajes Win32 a `InputEvent`
pub struct InputDispatcher;

impl InputDispatcher {
    /// Mapea un mensaje de ventana Win32 a un `InputEvent` tipado
    pub fn translate_win32_message(
        msg: u32,
        wparam: usize,
        lparam: isize,
    ) -> Option<InputEvent> {
        let modifiers = KeyModifiers::query_current_win32();

        match msg {
            WM_KEYDOWN => Some(InputEvent::KeyDown {
                key_code: wparam as u32,
                modifiers,
            }),
            WM_KEYUP => Some(InputEvent::KeyUp {
                key_code: wparam as u32,
                modifiers,
            }),
            // Enter, Tab, Backspace y Esc llegan también como WM_CHAR de control: viajan como KeyDown/KeyUp
            WM_CHAR => char::from_u32(wparam as u32)
                .filter(|c| !c.is_control())
                .map(|scalar| InputEvent::Char { scalar }),
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
                let (x, y) = parse_mouse_coords(lparam);
                Some(InputEvent::MouseDown {
                    button: mouse_button(msg),
                    x,
                    y,
                    modifiers,
                })
            }
            WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
                let (x, y) = parse_mouse_coords(lparam);
                Some(InputEvent::MouseUp {
                    button: mouse_button(msg),
                    x,
                    y,
                    modifiers,
                })
            }
            WM_MOUSEMOVE => {
                let (x, y) = parse_mouse_coords(lparam);
                Some(InputEvent::MouseMove { x, y, modifiers })
            }
            WM_MOUSEWHEEL => {
                let (x, y) = parse_mouse_coords(lparam);
                let raw_delta = ((wparam >> 16) as i16) as f32;
                let delta_y = raw_delta / 120.0; // 120 unidades por muesca estándar de rueda Win32
                Some(InputEvent::MouseWheel {
                    delta_x: 0.0,
                    delta_y,
                    x,
                    y,
                    modifiers,
                })
            }
            WM_SIZE => {
                let width = (lparam & 0xFFFF) as u32;
                let height = ((lparam >> 16) & 0xFFFF) as u32;
                Some(InputEvent::Resize { width, height })
            }
            _ => None,
        }
    }
}

/// Botón del ratón de un mensaje WM_*BUTTON*
fn mouse_button(msg: u32) -> MouseButton {
    match msg {
        WM_RBUTTONDOWN | WM_RBUTTONUP => MouseButton::Right,
        WM_MBUTTONDOWN | WM_MBUTTONUP => MouseButton::Middle,
        _ => MouseButton::Left,
    }
}

/// Extrae coordenadas (x, y) con signo desde el lParam de un mensaje de ratón Win32
#[inline]
fn parse_mouse_coords(lparam: isize) -> (i32, i32) {
    let x = (lparam as i16) as i32;
    let y = ((lparam >> 16) as i16) as i32;
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_win32_keyboard_translation() {
        let key_down = InputDispatcher::translate_win32_message(WM_KEYDOWN, 0x41, 0);
        assert!(key_down.is_some());
        if let Some(InputEvent::KeyDown { key_code, .. }) = key_down {
            assert_eq!(key_code, 0x41); // 'A'
        } else {
            panic!("Esperado KeyDown");
        }

        let key_up = InputDispatcher::translate_win32_message(WM_KEYUP, 0x41, 0);
        assert!(key_up.is_some());
        if let Some(InputEvent::KeyUp { key_code, .. }) = key_up {
            assert_eq!(key_code, 0x41);
        } else {
            panic!("Esperado KeyUp");
        }
    }

    #[test]
    fn test_win32_mouse_translation() {
        // Coordenadas: x=150, y=300 -> lParam = (300 << 16) | 150 = 19660950
        let lparam: isize = (300 << 16) | 150;

        let mouse_down = InputDispatcher::translate_win32_message(WM_LBUTTONDOWN, 0, lparam);
        assert!(mouse_down.is_some());
        if let Some(InputEvent::MouseDown { button, x, y, .. }) = mouse_down {
            assert_eq!(button, MouseButton::Left);
            assert_eq!(x, 150);
            assert_eq!(y, 300);
        } else {
            panic!("Esperado MouseDown");
        }

        let mouse_move = InputDispatcher::translate_win32_message(WM_MOUSEMOVE, 0, lparam);
        assert!(mouse_move.is_some());
        if let Some(InputEvent::MouseMove { x, y, .. }) = mouse_move {
            assert_eq!(x, 150);
            assert_eq!(y, 300);
        } else {
            panic!("Esperado MouseMove");
        }

        // Rueda de ratón: 1 muesca hacia arriba (+120) -> wparam = 120 << 16
        let wparam: usize = (120usize & 0xFFFF) << 16;
        let wheel = InputDispatcher::translate_win32_message(WM_MOUSEWHEEL, wparam, lparam);
        assert!(wheel.is_some());
        if let Some(InputEvent::MouseWheel { delta_y, x, y, .. }) = wheel {
            assert_eq!(delta_y, 1.0);
            assert_eq!(x, 150);
            assert_eq!(y, 300);
        } else {
            panic!("Esperado MouseWheel");
        }
    }

    /// R-03: los seis mensajes de botón se siguen mapeando al botón y la acción correctos
    #[test]
    fn test_win32_mouse_buttons_inc20261025() {
        for (msg, button, down) in [
            (WM_LBUTTONDOWN, MouseButton::Left, true),
            (WM_LBUTTONUP, MouseButton::Left, false),
            (WM_RBUTTONDOWN, MouseButton::Right, true),
            (WM_RBUTTONUP, MouseButton::Right, false),
            (WM_MBUTTONDOWN, MouseButton::Middle, true),
            (WM_MBUTTONUP, MouseButton::Middle, false),
        ] {
            match InputDispatcher::translate_win32_message(msg, 0, (2 << 16) | 1) {
                Some(InputEvent::MouseDown { button: b, x: 1, y: 2, .. }) if down => assert_eq!(b, button),
                Some(InputEvent::MouseUp { button: b, x: 1, y: 2, .. }) if !down => assert_eq!(b, button),
                other => panic!("{:#x}: {:?}", msg, other),
            }
        }
    }

    /// R-03: la máscara de modificadores conserva el formato de cable que decodifica el worker
    #[test]
    fn test_modifier_bitfield_wire_format_inc20261025() {
        let m = |shift, ctrl, alt, meta| KeyModifiers { shift, ctrl, alt, meta }.to_bitfield();
        assert_eq!(m(false, false, false, false), 0);
        assert_eq!(m(true, false, false, false), 1);
        assert_eq!(m(false, true, false, false), 2);
        assert_eq!(m(false, false, true, false), 4);
        assert_eq!(m(false, false, false, true), 8);
        assert_eq!(m(true, true, true, true), 15);
    }

    #[test]
    fn test_win32_char_translation_delta02() {
        assert_eq!(
            InputDispatcher::translate_win32_message(WM_CHAR, 'ñ' as usize, 0),
            Some(InputEvent::Char { scalar: 'ñ' })
        );
        assert_eq!(
            InputDispatcher::translate_win32_message(WM_CHAR, 'A' as usize, 0),
            Some(InputEvent::Char { scalar: 'A' })
        );
        // Control (Enter \r, Tab, Backspace, Esc, DEL) y mitades de par sustituto: sin evento de texto
        for w in [0x0Dusize, 0x09, 0x08, 0x1B, 0x7F, 0xD83D, 0xDE00] {
            assert_eq!(InputDispatcher::translate_win32_message(WM_CHAR, w, 0), None, "{:#x}", w);
        }
    }

    #[test]
    fn test_win32_resize_translation() {
        // Ancho=1024, Alto=768 -> lParam = (768 << 16) | 1024
        let lparam: isize = (768 << 16) | 1024;
        let resize = InputDispatcher::translate_win32_message(WM_SIZE, 0, lparam);
        assert!(resize.is_some());
        if let Some(InputEvent::Resize { width, height }) = resize {
            assert_eq!(width, 1024);
            assert_eq!(height, 768);
        } else {
            panic!("Esperado Resize");
        }
    }
}
