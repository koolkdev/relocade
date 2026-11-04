use super::super::{KeyStroke, ShiftState};
use relocade_frontend::keyboard::KeyCode;

/// The default guest layout is the US PC keyboard, independent of host text.
pub(super) fn keystroke(code: KeyCode, shifts: ShiftState) -> Option<KeyStroke> {
    use KeyCode::*;
    let scan_code = scan_code(code)?;
    let shift = shifts.left_shift || shifts.right_shift;
    let mut character = match code {
        Enter | NumpadEnter => b'\r',
        Escape => 27,
        Backspace => 8,
        Tab => b'\t',
        Space => b' ',
        NumpadDivide => b'/',
        NumpadMultiply => b'*',
        NumpadSubtract => b'-',
        NumpadAdd => b'+',
        Numpad0 | Numpad1 | Numpad2 | Numpad3 | Numpad4 | Numpad5 | Numpad6 | Numpad7 | Numpad8
        | Numpad9 | NumpadDecimal
            if shifts.num_lock ^ shift =>
        {
            match code {
                Numpad0 => b'0',
                Numpad1 => b'1',
                Numpad2 => b'2',
                Numpad3 => b'3',
                Numpad4 => b'4',
                Numpad5 => b'5',
                Numpad6 => b'6',
                Numpad7 => b'7',
                Numpad8 => b'8',
                Numpad9 => b'9',
                _ => b'.',
            }
        }
        _ => match scan_code {
            0x02..=0x0d => b"1234567890-="[usize::from(scan_code - 2)],
            0x10..=0x1b => b"qwertyuiop[]"[usize::from(scan_code - 0x10)],
            0x1e..=0x29 => b"asdfghjkl;'`"[usize::from(scan_code - 0x1e)],
            0x2b => b'\\',
            0x2c..=0x35 => b"zxcvbnm,./"[usize::from(scan_code - 0x2c)],
            _ => 0,
        },
    };
    if shifts.alt {
        character = 0;
    } else if shifts.control {
        character = match character {
            b'a'..=b'z' | b'['..=b']' => character & 0x1f,
            b'2' | b' ' => 0,
            b'6' => 30,
            b'-' => 31,
            8 => 127,
            b'\r' => b'\n',
            _ => character,
        };
    } else if character.is_ascii_alphabetic() {
        if shift ^ shifts.caps_lock {
            character = character.to_ascii_uppercase();
        }
    } else if shift {
        // Keypad punctuation is unaffected by Shift.
        character = match code {
            Digit0 | Digit1 | Digit2 | Digit3 | Digit4 | Digit5 | Digit6 | Digit7 | Digit8
            | Digit9 | Minus | Equal => b"!@#$%^&*()_+"[usize::from(scan_code - 2)],
            BracketLeft => b'{',
            BracketRight => b'}',
            Semicolon => b':',
            Quote => b'"',
            Backquote => b'~',
            Backslash => b'|',
            Comma => b'<',
            Period => b'>',
            Slash => b'?',
            Tab => 0,
            _ => character,
        };
    }
    Some(KeyStroke {
        character,
        scan_code,
    })
}

/// BIOS base scan codes for the supported PC keys, including BIOS F11/F12 codes.
pub(super) fn scan_code(code: KeyCode) -> Option<u8> {
    use KeyCode::*;
    Some(match code {
        Escape => 0x01,
        Digit1 => 0x02,
        Digit2 => 0x03,
        Digit3 => 0x04,
        Digit4 => 0x05,
        Digit5 => 0x06,
        Digit6 => 0x07,
        Digit7 => 0x08,
        Digit8 => 0x09,
        Digit9 => 0x0a,
        Digit0 => 0x0b,
        Minus => 0x0c,
        Equal => 0x0d,
        Backspace => 0x0e,
        Tab => 0x0f,
        KeyQ => 0x10,
        KeyW => 0x11,
        KeyE => 0x12,
        KeyR => 0x13,
        KeyT => 0x14,
        KeyY => 0x15,
        KeyU => 0x16,
        KeyI => 0x17,
        KeyO => 0x18,
        KeyP => 0x19,
        BracketLeft => 0x1a,
        BracketRight => 0x1b,
        Enter | NumpadEnter => 0x1c,
        KeyA => 0x1e,
        KeyS => 0x1f,
        KeyD => 0x20,
        KeyF => 0x21,
        KeyG => 0x22,
        KeyH => 0x23,
        KeyJ => 0x24,
        KeyK => 0x25,
        KeyL => 0x26,
        Semicolon => 0x27,
        Quote => 0x28,
        Backquote => 0x29,
        Backslash => 0x2b,
        KeyZ => 0x2c,
        KeyX => 0x2d,
        KeyC => 0x2e,
        KeyV => 0x2f,
        KeyB => 0x30,
        KeyN => 0x31,
        KeyM => 0x32,
        Comma => 0x33,
        Period => 0x34,
        Slash | NumpadDivide => 0x35,
        NumpadMultiply => 0x37,
        Space => 0x39,
        F1 => 0x3b,
        F2 => 0x3c,
        F3 => 0x3d,
        F4 => 0x3e,
        F5 => 0x3f,
        F6 => 0x40,
        F7 => 0x41,
        F8 => 0x42,
        F9 => 0x43,
        F10 => 0x44,
        Home | Numpad7 => 0x47,
        ArrowUp | Numpad8 => 0x48,
        PageUp | Numpad9 => 0x49,
        NumpadSubtract => 0x4a,
        ArrowLeft | Numpad4 => 0x4b,
        Numpad5 => 0x4c,
        ArrowRight | Numpad6 => 0x4d,
        NumpadAdd => 0x4e,
        End | Numpad1 => 0x4f,
        ArrowDown | Numpad2 => 0x50,
        PageDown | Numpad3 => 0x51,
        Insert | Numpad0 => 0x52,
        Delete | NumpadDecimal => 0x53,
        F11 => 0x85,
        F12 => 0x86,
        _ => return None,
    })
}
