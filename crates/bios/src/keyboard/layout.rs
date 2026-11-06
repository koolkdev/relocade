//! Shared US BIOS translation for physical events and decoded set-1 keys.
//!
//! Hardware make codes identify keys; BIOS scan codes identify translated results.
//! The tables below spell out those results instead of computing offsets between
//! unrelated BIOS code ranges. Source: IBM PC/AT Technical Reference (March 1986),
//! KYBD and ORGS keyboard tables; see README.md for the supported subset.

use super::{KeyStroke, ShiftState};
use relocade_frontend::keyboard::KeyCode;

const EXTENDED_KEY: u16 = 0xe000;

// Map physical identities and wire numbers together. E0-prefixed keys stay
// distinct: Enter and keypad Enter, or keypad 7 and Home, translate differently.
macro_rules! keys {
    ($($name:ident = $code:literal => $physical:ident),* $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq)]
        #[repr(u16)]
        pub(super) enum Key { $($name = $code),* }

        impl Key {
            pub fn from_scan(code: u8, extended: bool) -> Option<Self> {
                let code = u16::from(code) | if extended { EXTENDED_KEY } else { 0 };
                match code { $($code => Some(Self::$name),)* _ => None }
            }

            pub fn from_physical(code: KeyCode) -> Option<Self> {
                match code { $(KeyCode::$physical => Some(Self::$name),)* _ => None }
            }

            /// The unprefixed hardware make code, also used by BIOS for ordinary keys.
            pub fn make_code(self) -> u8 { self as u16 as u8 }
        }
    };
}

keys! {
    Escape = 0x01 => Escape,
    Digit1 = 0x02 => Digit1,
    Digit2 = 0x03 => Digit2,
    Digit3 = 0x04 => Digit3,
    Digit4 = 0x05 => Digit4,
    Digit5 = 0x06 => Digit5,
    Digit6 = 0x07 => Digit6,
    Digit7 = 0x08 => Digit7,
    Digit8 = 0x09 => Digit8,
    Digit9 = 0x0a => Digit9,
    Digit0 = 0x0b => Digit0,
    Minus = 0x0c => Minus,
    Equal = 0x0d => Equal,
    Backspace = 0x0e => Backspace,
    Tab = 0x0f => Tab,
    Q = 0x10 => KeyQ,
    W = 0x11 => KeyW,
    E = 0x12 => KeyE,
    R = 0x13 => KeyR,
    T = 0x14 => KeyT,
    Y = 0x15 => KeyY,
    U = 0x16 => KeyU,
    I = 0x17 => KeyI,
    O = 0x18 => KeyO,
    P = 0x19 => KeyP,
    LeftBracket = 0x1a => BracketLeft,
    RightBracket = 0x1b => BracketRight,
    Enter = 0x1c => Enter,
    LeftControl = 0x1d => ControlLeft,
    A = 0x1e => KeyA,
    S = 0x1f => KeyS,
    D = 0x20 => KeyD,
    F = 0x21 => KeyF,
    G = 0x22 => KeyG,
    H = 0x23 => KeyH,
    J = 0x24 => KeyJ,
    K = 0x25 => KeyK,
    L = 0x26 => KeyL,
    Semicolon = 0x27 => Semicolon,
    Quote = 0x28 => Quote,
    Backquote = 0x29 => Backquote,
    LeftShift = 0x2a => ShiftLeft,
    Backslash = 0x2b => Backslash,
    Z = 0x2c => KeyZ,
    X = 0x2d => KeyX,
    C = 0x2e => KeyC,
    V = 0x2f => KeyV,
    B = 0x30 => KeyB,
    N = 0x31 => KeyN,
    M = 0x32 => KeyM,
    Comma = 0x33 => Comma,
    Period = 0x34 => Period,
    Slash = 0x35 => Slash,
    RightShift = 0x36 => ShiftRight,
    KeypadMultiply = 0x37 => NumpadMultiply,
    LeftAlt = 0x38 => AltLeft,
    Space = 0x39 => Space,
    CapsLock = 0x3a => CapsLock,
    F1 = 0x3b => F1,
    F2 = 0x3c => F2,
    F3 = 0x3d => F3,
    F4 = 0x3e => F4,
    F5 = 0x3f => F5,
    F6 = 0x40 => F6,
    F7 = 0x41 => F7,
    F8 = 0x42 => F8,
    F9 = 0x43 => F9,
    F10 = 0x44 => F10,
    NumLock = 0x45 => NumLock,
    ScrollLock = 0x46 => ScrollLock,
    Keypad7 = 0x47 => Numpad7,
    Keypad8 = 0x48 => Numpad8,
    Keypad9 = 0x49 => Numpad9,
    KeypadSubtract = 0x4a => NumpadSubtract,
    Keypad4 = 0x4b => Numpad4,
    Keypad5 = 0x4c => Numpad5,
    Keypad6 = 0x4d => Numpad6,
    KeypadAdd = 0x4e => NumpadAdd,
    Keypad1 = 0x4f => Numpad1,
    Keypad2 = 0x50 => Numpad2,
    Keypad3 = 0x51 => Numpad3,
    Keypad0 = 0x52 => Numpad0,
    KeypadDecimal = 0x53 => NumpadDecimal,
    IntlBackslash = 0x56 => IntlBackslash,
    F11 = 0x57 => F11,
    F12 = 0x58 => F12,
    KeypadEnter = 0xe01c => NumpadEnter,
    RightControl = 0xe01d => ControlRight,
    KeypadDivide = 0xe035 => NumpadDivide,
    RightAlt = 0xe038 => AltRight,
    Home = 0xe047 => Home,
    Up = 0xe048 => ArrowUp,
    PageUp = 0xe049 => PageUp,
    Left = 0xe04b => ArrowLeft,
    Right = 0xe04d => ArrowRight,
    End = 0xe04f => End,
    Down = 0xe050 => ArrowDown,
    PageDown = 0xe051 => PageDown,
    Insert = 0xe052 => Insert,
    Delete = 0xe053 => Delete,
}

pub(super) fn keystroke(key: Key, shifts: ShiftState) -> Option<KeyStroke> {
    use Key::*;
    // BIOS scan-code columns: unmodified, Shift, Ctrl, Alt.
    let function = match key {
        F1 => Some([0x3b, 0x54, 0x5e, 0x68]),
        F2 => Some([0x3c, 0x55, 0x5f, 0x69]),
        F3 => Some([0x3d, 0x56, 0x60, 0x6a]),
        F4 => Some([0x3e, 0x57, 0x61, 0x6b]),
        F5 => Some([0x3f, 0x58, 0x62, 0x6c]),
        F6 => Some([0x40, 0x59, 0x63, 0x6d]),
        F7 => Some([0x41, 0x5a, 0x64, 0x6e]),
        F8 => Some([0x42, 0x5b, 0x65, 0x6f]),
        F9 => Some([0x43, 0x5c, 0x66, 0x70]),
        F10 => Some([0x44, 0x5d, 0x67, 0x71]),
        F11 => Some([0x85, 0x87, 0x89, 0x8b]),
        F12 => Some([0x86, 0x88, 0x8a, 0x8c]),
        _ => None,
    };
    if let Some(codes) = function {
        return Some(function_key(codes, shifts));
    }

    let navigation = match key {
        Home | Keypad7 => Some([0x47, 0x47, 0x77, 0x97]),
        Up | Keypad8 => Some([0x48, 0x48, 0x8d, 0x98]),
        PageUp | Keypad9 => Some([0x49, 0x49, 0x84, 0x99]),
        Left | Keypad4 => Some([0x4b, 0x4b, 0x73, 0x9b]),
        Keypad5 => Some([0x4c, 0x4c, 0x8f, 0]),
        Right | Keypad6 => Some([0x4d, 0x4d, 0x74, 0x9d]),
        End | Keypad1 => Some([0x4f, 0x4f, 0x75, 0x9f]),
        Down | Keypad2 => Some([0x50, 0x50, 0x91, 0xa0]),
        PageDown | Keypad3 => Some([0x51, 0x51, 0x76, 0xa1]),
        Insert | Keypad0 => Some([0x52, 0x52, 0x92, 0xa2]),
        Delete | KeypadDecimal => Some([0x53, 0x53, 0x93, 0xa3]),
        _ => None,
    };
    if let Some(codes) = navigation {
        let digit = match key {
            Keypad7 => Some(b'7'),
            Keypad8 => Some(b'8'),
            Keypad9 => Some(b'9'),
            Keypad4 => Some(b'4'),
            Keypad5 => Some(b'5'),
            Keypad6 => Some(b'6'),
            Keypad1 => Some(b'1'),
            Keypad2 => Some(b'2'),
            Keypad3 => Some(b'3'),
            Keypad0 => Some(b'0'),
            KeypadDecimal => Some(b'.'),
            _ => None,
        };
        if shifts.alt && digit.is_some() {
            // Alt-numpad composition is not implemented; do not emit navigation.
            return None;
        }
        if !shifts.alt && !shifts.control && shifts.numeric_keypad() {
            if let Some(character) = digit {
                return Some(character_key(key, character));
            }
        }
        return Some(function_key(codes, shifts));
    }

    // Character columns: unmodified, Shift. CapsLock affects only letters.
    #[allow(clippy::byte_char_slices)] // Separate normal/Shift columns, not byte strings.
    let [plain, shifted] = match key {
        Escape => [b'\x1b', b'\x1b'],
        Digit1 => [b'1', b'!'],
        Digit2 => [b'2', b'@'],
        Digit3 => [b'3', b'#'],
        Digit4 => [b'4', b'$'],
        Digit5 => [b'5', b'%'],
        Digit6 => [b'6', b'^'],
        Digit7 => [b'7', b'&'],
        Digit8 => [b'8', b'*'],
        Digit9 => [b'9', b'('],
        Digit0 => [b'0', b')'],
        Minus => [b'-', b'_'],
        Equal => [b'=', b'+'],
        Backspace => [b'\x08', b'\x08'],
        Tab => [b'\t', 0],
        Q => [b'q', b'Q'],
        W => [b'w', b'W'],
        E => [b'e', b'E'],
        R => [b'r', b'R'],
        T => [b't', b'T'],
        Y => [b'y', b'Y'],
        U => [b'u', b'U'],
        I => [b'i', b'I'],
        O => [b'o', b'O'],
        P => [b'p', b'P'],
        LeftBracket => [b'[', b'{'],
        RightBracket => [b']', b'}'],
        Enter | KeypadEnter => [b'\r', b'\r'],
        A => [b'a', b'A'],
        S => [b's', b'S'],
        D => [b'd', b'D'],
        F => [b'f', b'F'],
        G => [b'g', b'G'],
        H => [b'h', b'H'],
        J => [b'j', b'J'],
        K => [b'k', b'K'],
        L => [b'l', b'L'],
        Semicolon => [b';', b':'],
        Quote => [b'\'', b'"'],
        Backquote => [b'`', b'~'],
        Backslash | IntlBackslash => [b'\\', b'|'],
        Z => [b'z', b'Z'],
        X => [b'x', b'X'],
        C => [b'c', b'C'],
        V => [b'v', b'V'],
        B => [b'b', b'B'],
        N => [b'n', b'N'],
        M => [b'm', b'M'],
        Comma => [b',', b'<'],
        Period => [b'.', b'>'],
        Slash => [b'/', b'?'],
        KeypadDivide => [b'/', b'/'],
        KeypadMultiply => [b'*', b'*'],
        KeypadSubtract => [b'-', b'-'],
        KeypadAdd => [b'+', b'+'],
        Space => [b' ', b' '],
        _ => return None, // Modifiers and lock keys do not produce keystrokes.
    };
    if shifts.alt {
        if key == Space {
            return Some(character_key(key, b' '));
        }
        // BIOS gives the Alt number row and enhanced keys their own scan codes.
        let scan_code = match key {
            Digit1 => 0x78,
            Digit2 => 0x79,
            Digit3 => 0x7a,
            Digit4 => 0x7b,
            Digit5 => 0x7c,
            Digit6 => 0x7d,
            Digit7 => 0x7e,
            Digit8 => 0x7f,
            Digit9 => 0x80,
            Digit0 => 0x81,
            Minus => 0x82,
            Equal => 0x83,
            Tab => 0xa5,
            KeypadEnter => 0xa6,
            KeypadDivide => 0xa4,
            _ => key.make_code(),
        };
        return Some(special_key(scan_code));
    }
    if shifts.control {
        let special = match key {
            Tab => Some(0x94),
            KeypadDivide => Some(0x95),
            KeypadMultiply => Some(0x96),
            KeypadSubtract => Some(0x8e),
            KeypadAdd => Some(0x90),
            _ => None,
        };
        if let Some(scan_code) = special {
            return Some(special_key(scan_code));
        }
        let character = match key {
            Escape => b'\x1b',
            Backspace => b'\x7f',
            Enter | KeypadEnter => b'\n',
            Digit2 => 0,
            Digit6 => b'\x1e',
            Minus => b'\x1f',
            Space => b' ',
            _ if plain.is_ascii_alphabetic()
                || matches!(key, LeftBracket | Backslash | RightBracket) =>
            {
                // ASCII Ctrl+A..Ctrl+Z and Ctrl+[ / Ctrl+\ / Ctrl+].
                plain.to_ascii_uppercase() - b'@'
            }
            _ => return None, // Unsupported Ctrl combinations are ignored by BIOS.
        };
        return Some(character_key(key, character));
    }
    let uppercase = shifts.shift_pressed() ^ (shifts.caps_lock && plain.is_ascii_alphabetic());
    Some(character_key(key, if uppercase { shifted } else { plain }))
}

fn character_key(key: Key, character: u8) -> KeyStroke {
    KeyStroke {
        character,
        scan_code: key.make_code(),
    }
}

fn special_key(scan_code: u8) -> KeyStroke {
    KeyStroke {
        character: 0,
        scan_code,
    }
}

fn function_key([plain, shift, control, alt]: [u8; 4], shifts: ShiftState) -> KeyStroke {
    special_key(if shifts.alt {
        alt
    } else if shifts.control {
        control
    } else if shifts.shift_pressed() {
        shift
    } else {
        plain
    })
}
