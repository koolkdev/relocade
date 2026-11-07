use relocade_frontend::keyboard::KeyCode;

// IBM PC/AT Technical Reference, March 1986, pp. 1-46.1–1-46.3 and
// 4-49–4-57: physical key positions, keyboard set 2 and translated set 1.
// Tuples retain both wire codes; BIOS character translation is a separate owner.
const KEYS: &[(KeyCode, u8, u8, bool)] = &[
    (KeyCode::Escape, 0x01, 0x76, false),
    (KeyCode::Digit1, 0x02, 0x16, false),
    (KeyCode::Digit2, 0x03, 0x1e, false),
    (KeyCode::Digit3, 0x04, 0x26, false),
    (KeyCode::Digit4, 0x05, 0x25, false),
    (KeyCode::Digit5, 0x06, 0x2e, false),
    (KeyCode::Digit6, 0x07, 0x36, false),
    (KeyCode::Digit7, 0x08, 0x3d, false),
    (KeyCode::Digit8, 0x09, 0x3e, false),
    (KeyCode::Digit9, 0x0a, 0x46, false),
    (KeyCode::Digit0, 0x0b, 0x45, false),
    (KeyCode::Minus, 0x0c, 0x4e, false),
    (KeyCode::Equal, 0x0d, 0x55, false),
    (KeyCode::Backspace, 0x0e, 0x66, false),
    (KeyCode::Tab, 0x0f, 0x0d, false),
    (KeyCode::KeyQ, 0x10, 0x15, false),
    (KeyCode::KeyW, 0x11, 0x1d, false),
    (KeyCode::KeyE, 0x12, 0x24, false),
    (KeyCode::KeyR, 0x13, 0x2d, false),
    (KeyCode::KeyT, 0x14, 0x2c, false),
    (KeyCode::KeyY, 0x15, 0x35, false),
    (KeyCode::KeyU, 0x16, 0x3c, false),
    (KeyCode::KeyI, 0x17, 0x43, false),
    (KeyCode::KeyO, 0x18, 0x44, false),
    (KeyCode::KeyP, 0x19, 0x4d, false),
    (KeyCode::BracketLeft, 0x1a, 0x54, false),
    (KeyCode::BracketRight, 0x1b, 0x56, false),
    (KeyCode::Enter, 0x1c, 0x5a, false),
    (KeyCode::NumpadEnter, 0x1c, 0x5a, true),
    (KeyCode::KeyA, 0x1e, 0x1c, false),
    (KeyCode::KeyS, 0x1f, 0x1b, false),
    (KeyCode::KeyD, 0x20, 0x23, false),
    (KeyCode::KeyF, 0x21, 0x2b, false),
    (KeyCode::KeyG, 0x22, 0x34, false),
    (KeyCode::KeyH, 0x23, 0x33, false),
    (KeyCode::KeyJ, 0x24, 0x3b, false),
    (KeyCode::KeyK, 0x25, 0x42, false),
    (KeyCode::KeyL, 0x26, 0x4b, false),
    (KeyCode::Semicolon, 0x27, 0x4c, false),
    (KeyCode::Quote, 0x28, 0x52, false),
    (KeyCode::Backquote, 0x29, 0x0e, false),
    (KeyCode::Backslash, 0x2b, 0x5d, false),
    (KeyCode::KeyZ, 0x2c, 0x1a, false),
    (KeyCode::KeyX, 0x2d, 0x22, false),
    (KeyCode::KeyC, 0x2e, 0x21, false),
    (KeyCode::KeyV, 0x2f, 0x2a, false),
    (KeyCode::KeyB, 0x30, 0x32, false),
    (KeyCode::KeyN, 0x31, 0x31, false),
    (KeyCode::KeyM, 0x32, 0x3a, false),
    (KeyCode::Comma, 0x33, 0x41, false),
    (KeyCode::Period, 0x34, 0x49, false),
    (KeyCode::Slash, 0x35, 0x4a, false),
    (KeyCode::NumpadDivide, 0x35, 0x4a, true),
    (KeyCode::NumpadMultiply, 0x37, 0x7c, false),
    (KeyCode::Space, 0x39, 0x29, false),
    (KeyCode::F1, 0x3b, 0x05, false),
    (KeyCode::F2, 0x3c, 0x06, false),
    (KeyCode::F3, 0x3d, 0x04, false),
    (KeyCode::F4, 0x3e, 0x0c, false),
    (KeyCode::F5, 0x3f, 0x03, false),
    (KeyCode::F6, 0x40, 0x0b, false),
    (KeyCode::F7, 0x41, 0x83, false),
    (KeyCode::F8, 0x42, 0x0a, false),
    (KeyCode::F9, 0x43, 0x01, false),
    (KeyCode::F10, 0x44, 0x09, false),
    (KeyCode::Home, 0x47, 0x6c, true),
    (KeyCode::Numpad7, 0x47, 0x6c, false),
    (KeyCode::ArrowUp, 0x48, 0x75, true),
    (KeyCode::Numpad8, 0x48, 0x75, false),
    (KeyCode::PageUp, 0x49, 0x7d, true),
    (KeyCode::Numpad9, 0x49, 0x7d, false),
    (KeyCode::NumpadSubtract, 0x4a, 0x7b, false),
    (KeyCode::ArrowLeft, 0x4b, 0x6b, true),
    (KeyCode::Numpad4, 0x4b, 0x6b, false),
    (KeyCode::Numpad5, 0x4c, 0x73, false),
    (KeyCode::ArrowRight, 0x4d, 0x74, true),
    (KeyCode::Numpad6, 0x4d, 0x74, false),
    (KeyCode::NumpadAdd, 0x4e, 0x79, false),
    (KeyCode::End, 0x4f, 0x69, true),
    (KeyCode::Numpad1, 0x4f, 0x69, false),
    (KeyCode::ArrowDown, 0x50, 0x72, true),
    (KeyCode::Numpad2, 0x50, 0x72, false),
    (KeyCode::PageDown, 0x51, 0x7a, true),
    (KeyCode::Numpad3, 0x51, 0x7a, false),
    (KeyCode::Insert, 0x52, 0x70, true),
    (KeyCode::Numpad0, 0x52, 0x70, false),
    (KeyCode::Delete, 0x53, 0x71, true),
    (KeyCode::NumpadDecimal, 0x53, 0x71, false),
    (KeyCode::F11, 0x57, 0x78, false),
    (KeyCode::F12, 0x58, 0x07, false),
    (KeyCode::ShiftLeft, 0x2a, 0x12, false),
    (KeyCode::ShiftRight, 0x36, 0x59, false),
    (KeyCode::ControlLeft, 0x1d, 0x14, false),
    (KeyCode::ControlRight, 0x1d, 0x14, true),
    (KeyCode::AltLeft, 0x38, 0x11, false),
    (KeyCode::AltRight, 0x38, 0x11, true),
    (KeyCode::CapsLock, 0x3a, 0x58, false),
    (KeyCode::NumLock, 0x45, 0x77, false),
    (KeyCode::ScrollLock, 0x46, 0x7e, false),
    (KeyCode::IntlBackslash, 0x56, 0x61, false),
    (KeyCode::SuperLeft, 0x5b, 0x1f, true),
    (KeyCode::SuperRight, 0x5c, 0x27, true),
    (KeyCode::ContextMenu, 0x5d, 0x2f, true),
];

pub(super) fn supported(key: KeyCode) -> bool {
    matches!(key, KeyCode::PrintScreen | KeyCode::Pause)
        || KEYS.iter().any(|&(code, ..)| code == key)
}

pub(super) fn repeatable(key: KeyCode) -> bool {
    key != KeyCode::Pause
}

pub(super) fn sequence(
    key: KeyCode,
    pressed: bool,
    set: u8,
    held: &std::collections::BTreeSet<KeyCode>,
) -> Vec<u8> {
    use KeyCode::*;
    let control = held.contains(&ControlLeft) || held.contains(&ControlRight);
    let alt = held.contains(&AltLeft) || held.contains(&AltRight);
    let shift = held.contains(&ShiftLeft) || held.contains(&ShiftRight);
    if key == Pause {
        return if !pressed {
            vec![]
        } else if control {
            if set == 1 {
                vec![0xe0, 0x46, 0xe0, 0xc6]
            } else {
                vec![0xe0, 0x7e, 0xe0, 0xf0, 0x7e]
            }
        } else if set == 1 {
            vec![0xe1, 0x1d, 0x45, 0xe1, 0x9d, 0xc5]
        } else {
            vec![0xe1, 0x14, 0x77, 0xe1, 0xf0, 0x14, 0xf0, 0x77]
        };
    }
    if key == PrintScreen {
        if alt {
            return code_bytes(if set == 1 { 0x54 } else { 0x84 }, false, pressed, set);
        }
        let code = if set == 1 { 0x37 } else { 0x7c };
        let mut bytes = code_bytes(code, true, pressed, set);
        if !control && !shift {
            let fake_shift = code_bytes(if set == 1 { 0x2a } else { 0x12 }, true, pressed, set);
            if pressed {
                bytes.splice(..0, fake_shift);
            } else {
                bytes.extend(fake_shift);
            }
        }
        return bytes;
    }
    let Some(&(_, set1, set2, extended)) = KEYS.iter().find(|&&(code, ..)| code == key) else {
        return vec![];
    };
    code_bytes(if set == 1 { set1 } else { set2 }, extended, pressed, set)
}

fn code_bytes(code: u8, extended: bool, pressed: bool, set: u8) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(3);
    if extended {
        bytes.push(0xe0);
    }
    if !pressed && set == 2 {
        bytes.push(0xf0);
    }
    bytes.push(if !pressed && set == 1 {
        code | 0x80
    } else {
        code
    });
    bytes
}

/// Set-2 to set-1 translation also applies to keyboard replies (e.g. ID 83h).
pub(super) fn translate(byte: u8) -> u8 {
    match byte {
        0 => 0xff,
        2 => 0x41,
        0x84 => 0x54,
        _ => KEYS
            .iter()
            .find(|&&(_, _, set2, _)| set2 == byte)
            .map_or(byte, |&(_, set1, ..)| set1),
    }
}
