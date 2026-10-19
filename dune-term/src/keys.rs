//! Keyboard input to the bytes a terminal application expects (xterm conventions).

use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};

/// Bytes to send to the PTY for a key press, if any.
pub fn encode(sym: Keysym, utf8: Option<&str>, mods: &Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    // xterm modifier parameter: 1 + shift + 2*alt + 4*ctrl.
    let param = 1 + mods.shift as u8 + 2 * mods.alt as u8 + 4 * mods.ctrl as u8;

    // Cursor keys and Home/End: `ESC [ X`, `ESC O X` in application mode, `ESC [ 1 ; m X` with modifiers.
    let cursor = |c: char| {
        Some(if param > 1 {
            format!("\x1b[1;{param}{c}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{c}").into_bytes()
        } else {
            format!("\x1b[{c}").into_bytes()
        })
    };
    // Editing keys and F5+: `ESC [ n ~`, `ESC [ n ; m ~` with modifiers.
    let tilde = |n: u8| {
        Some(if param > 1 { format!("\x1b[{n};{param}~") } else { format!("\x1b[{n}~") }.into_bytes())
    };
    // F1–F4: `ESC O X`, `ESC [ 1 ; m X` with modifiers.
    let ss3 = |c: char| {
        Some(if param > 1 { format!("\x1b[1;{param}{c}") } else { format!("\x1bO{c}") }.into_bytes())
    };

    match sym {
        Keysym::Up => cursor('A'),
        Keysym::Down => cursor('B'),
        Keysym::Right => cursor('C'),
        Keysym::Left => cursor('D'),
        Keysym::Home => cursor('H'),
        Keysym::End => cursor('F'),
        Keysym::Insert => tilde(2),
        Keysym::Delete => tilde(3),
        Keysym::Page_Up => tilde(5),
        Keysym::Page_Down => tilde(6),
        Keysym::F1 => ss3('P'),
        Keysym::F2 => ss3('Q'),
        Keysym::F3 => ss3('R'),
        Keysym::F4 => ss3('S'),
        Keysym::F5 => tilde(15),
        Keysym::F6 => tilde(17),
        Keysym::F7 => tilde(18),
        Keysym::F8 => tilde(19),
        Keysym::F9 => tilde(20),
        Keysym::F10 => tilde(21),
        Keysym::F11 => tilde(23),
        Keysym::F12 => tilde(24),
        Keysym::ISO_Left_Tab => Some(b"\x1b[Z".to_vec()),
        Keysym::Return | Keysym::KP_Enter => Some(alt_prefix(mods, b"\r")),
        Keysym::BackSpace => Some(alt_prefix(mods, if mods.ctrl { b"\x08" } else { b"\x7f" })),
        Keysym::Tab => Some(alt_prefix(mods, b"\t")),
        Keysym::Escape => Some(alt_prefix(mods, b"\x1b")),
        _ => {
            // xkbcommon already turns Ctrl+letter into the control character.
            let text = utf8.filter(|t| !t.is_empty())?;
            Some(alt_prefix(mods, text.as_bytes()))
        }
    }
}

/// Keys the terminal handles itself instead of sending them to the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Copy,
    Paste,
    ScrollPageUp,
    ScrollPageDown,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

/// Ctrl+Shift+C / Ctrl+Shift+V, Shift+PageUp / Shift+PageDown, Ctrl+= / Ctrl+- / Ctrl+0.
/// Matched on keysyms, so they follow the keyboard layout.
pub fn shortcut(sym: Keysym, mods: &Modifiers) -> Option<Shortcut> {
    match sym {
        Keysym::c | Keysym::C if mods.ctrl && mods.shift => Some(Shortcut::Copy),
        Keysym::v | Keysym::V if mods.ctrl && mods.shift => Some(Shortcut::Paste),
        Keysym::Page_Up if mods.shift && !mods.ctrl => Some(Shortcut::ScrollPageUp),
        Keysym::Page_Down if mods.shift && !mods.ctrl => Some(Shortcut::ScrollPageDown),
        Keysym::equal | Keysym::plus if mods.ctrl => Some(Shortcut::ZoomIn),
        Keysym::minus | Keysym::underscore if mods.ctrl => Some(Shortcut::ZoomOut),
        Keysym::_0 if mods.ctrl => Some(Shortcut::ZoomReset),
        _ => None,
    }
}

/// An SGR (1006) mouse report for applications that asked for the mouse:
/// `button` 0/1/2 = left/middle/right, 64/65 = wheel up/down; `col`/`line` from 0.
pub fn mouse_sgr(button: u8, col: usize, line: usize, pressed: bool) -> Vec<u8> {
    format!("\x1b[<{button};{};{}{}", col + 1, line + 1, if pressed { 'M' } else { 'm' }).into_bytes()
}

/// Alt sends ESC before the key (the "meta sends escape" convention).
fn alt_prefix(mods: &Modifiers, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 1);
    if mods.alt {
        out.push(0x1b);
    }
    out.extend_from_slice(bytes);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(shift: bool, alt: bool, ctrl: bool) -> Modifiers {
        Modifiers { shift, alt, ctrl, ..Default::default() }
    }

    fn enc(sym: Keysym, utf8: Option<&str>, m: Modifiers, app: bool) -> Vec<u8> {
        encode(sym, utf8, &m, app).unwrap_or_default()
    }

    #[test]
    fn text_and_control() {
        let none = mods(false, false, false);
        assert_eq!(enc(Keysym::a, Some("a"), none, false), b"a");
        assert_eq!(enc(Keysym::c, Some("\x03"), mods(false, false, true), false), b"\x03");
        assert_eq!(enc(Keysym::x, Some("x"), mods(false, true, false), false), b"\x1bx");
        assert_eq!(enc(Keysym::ntilde, Some("ñ"), none, false), "ñ".as_bytes());
        assert_eq!(encode(Keysym::Shift_L, None, &none, false), None);
    }

    #[test]
    fn special_keys() {
        let none = mods(false, false, false);
        assert_eq!(enc(Keysym::Return, Some("\r"), none, false), b"\r");
        assert_eq!(enc(Keysym::BackSpace, Some("\x08"), none, false), b"\x7f");
        assert_eq!(enc(Keysym::BackSpace, None, mods(false, false, true), false), b"\x08");
        assert_eq!(enc(Keysym::ISO_Left_Tab, None, mods(true, false, false), false), b"\x1b[Z");
        assert_eq!(enc(Keysym::Delete, None, none, false), b"\x1b[3~");
        assert_eq!(enc(Keysym::F1, None, none, false), b"\x1bOP");
        assert_eq!(enc(Keysym::F5, None, none, false), b"\x1b[15~");
    }

    #[test]
    fn terminal_shortcuts() {
        let ctrl_shift = mods(true, false, true);
        assert_eq!(shortcut(Keysym::C, &ctrl_shift), Some(Shortcut::Copy));
        assert_eq!(shortcut(Keysym::V, &ctrl_shift), Some(Shortcut::Paste));
        assert_eq!(shortcut(Keysym::c, &mods(false, false, true)), None); // plain Ctrl+C is for the app
        assert_eq!(shortcut(Keysym::Page_Up, &mods(true, false, false)), Some(Shortcut::ScrollPageUp));
        assert_eq!(shortcut(Keysym::Page_Up, &mods(false, false, false)), None);
        assert_eq!(shortcut(Keysym::equal, &mods(false, false, true)), Some(Shortcut::ZoomIn));
        assert_eq!(shortcut(Keysym::plus, &ctrl_shift), Some(Shortcut::ZoomIn));
        assert_eq!(shortcut(Keysym::_0, &mods(false, false, true)), Some(Shortcut::ZoomReset));
    }

    #[test]
    fn sgr_mouse_reports() {
        assert_eq!(mouse_sgr(64, 0, 0, true), b"\x1b[<64;1;1M");
        assert_eq!(mouse_sgr(0, 9, 4, false), b"\x1b[<0;10;5m");
    }

    #[test]
    fn cursor_keys_and_modes() {
        let none = mods(false, false, false);
        assert_eq!(enc(Keysym::Up, None, none, false), b"\x1b[A");
        assert_eq!(enc(Keysym::Up, None, none, true), b"\x1bOA");
        assert_eq!(enc(Keysym::Left, None, mods(false, false, true), true), b"\x1b[1;5D");
        assert_eq!(enc(Keysym::End, None, mods(true, false, false), false), b"\x1b[1;2F");
        assert_eq!(enc(Keysym::Page_Up, None, mods(true, false, false), false), b"\x1b[5;2~");
    }
}
