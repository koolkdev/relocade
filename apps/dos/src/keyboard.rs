use relocade_app::Input;
use relocade_bios::{KeyStroke, Keyboard, ShiftState};
use winit::{
    event::ElementState,
    keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey},
};

pub(super) struct KeyboardInput {
    keyboard: Keyboard,
    shifts: ShiftState,
    modifiers: ModifiersState,
}
impl KeyboardInput {
    pub fn new(keyboard: Keyboard) -> Self {
        Self {
            keyboard,
            shifts: ShiftState::default(),
            modifiers: ModifiersState::empty(),
        }
    }
    pub fn handle(&mut self, input: Input) {
        match input {
            Input::Modifiers(modifiers) => self.modifiers = modifiers,
            Input::Focused(false) => {
                self.modifiers = ModifiersState::empty();
                self.shifts.left_shift = false;
                self.shifts.right_shift = false;
            }
            Input::Key(event) => {
                let pressed = event.state == ElementState::Pressed;
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::ShiftLeft) => self.shifts.left_shift = pressed,
                    PhysicalKey::Code(KeyCode::ShiftRight) => self.shifts.right_shift = pressed,
                    PhysicalKey::Code(code) if pressed && !event.repeat => match code {
                        KeyCode::CapsLock => self.shifts.caps_lock ^= true,
                        KeyCode::NumLock => self.shifts.num_lock ^= true,
                        KeyCode::ScrollLock => self.shifts.scroll_lock ^= true,
                        KeyCode::Insert => self.shifts.insert ^= true,
                        _ => {}
                    },
                    _ => {}
                }
                if pressed {
                    if let Some(key) =
                        translate(&event.logical_key, event.physical_key, self.modifiers)
                    {
                        self.keyboard.push(key);
                    }
                }
            }
            _ => {}
        }
        let mut shifts = self.shifts;
        shifts.control = self.modifiers.control_key();
        shifts.alt = self.modifiers.alt_key();
        // Aggregate state can arrive without the original left/right key press.
        shifts.left_shift |= self.modifiers.shift_key() && !shifts.right_shift;
        self.keyboard.set_shift_state(shifts);
    }
}

fn translate(key: &Key, physical: PhysicalKey, modifiers: ModifiersState) -> Option<KeyStroke> {
    let mut scan_code = match physical {
        PhysicalKey::Code(code) => scan_code(code),
        _ => 0,
    };
    let character = match key {
        Key::Character(text) => {
            let mut chars = text.chars();
            let character = chars.next()?;
            if chars.next().is_some() || !character.is_ascii() {
                return None;
            }
            if modifiers.alt_key() {
                scan_code = match physical {
                    PhysicalKey::Code(KeyCode::Digit1) => 0x78,
                    PhysicalKey::Code(KeyCode::Digit2) => 0x79,
                    PhysicalKey::Code(KeyCode::Digit3) => 0x7a,
                    PhysicalKey::Code(KeyCode::Digit4) => 0x7b,
                    PhysicalKey::Code(KeyCode::Digit5) => 0x7c,
                    PhysicalKey::Code(KeyCode::Digit6) => 0x7d,
                    PhysicalKey::Code(KeyCode::Digit7) => 0x7e,
                    PhysicalKey::Code(KeyCode::Digit8) => 0x7f,
                    PhysicalKey::Code(KeyCode::Digit9) => 0x80,
                    PhysicalKey::Code(KeyCode::Digit0) => 0x81,
                    PhysicalKey::Code(KeyCode::Minus) => 0x82,
                    PhysicalKey::Code(KeyCode::Equal) => 0x83,
                    _ if character.is_ascii_alphabetic() && scan_code != 0 => scan_code,
                    _ => return None,
                };
                0
            } else if modifiers.control_key() {
                match character.to_ascii_uppercase() {
                    '@'..='_' => character.to_ascii_uppercase() as u8 & 0x1f,
                    ' ' | '2' => 0,
                    '6' => 0x1e,
                    '-' => 0x1f,
                    _ => return None,
                }
            } else {
                character as u8
            }
        }
        Key::Named(NamedKey::Enter) if !modifiers.alt_key() => {
            if modifiers.control_key() {
                b'\n'
            } else {
                b'\r'
            }
        }
        Key::Named(NamedKey::Tab) if !modifiers.alt_key() && !modifiers.control_key() => {
            if modifiers.shift_key() {
                0
            } else {
                b'\t'
            }
        }
        Key::Named(NamedKey::Backspace) if !modifiers.alt_key() => {
            if modifiers.control_key() {
                127
            } else {
                8
            }
        }
        Key::Named(NamedKey::Escape) if !modifiers.alt_key() && !modifiers.control_key() => 27,
        Key::Named(NamedKey::Space) if !modifiers.alt_key() => {
            if modifiers.control_key() {
                0
            } else {
                b' '
            }
        }
        Key::Named(
            NamedKey::ArrowUp
            | NamedKey::ArrowDown
            | NamedKey::ArrowLeft
            | NamedKey::ArrowRight
            | NamedKey::Home
            | NamedKey::End
            | NamedKey::PageUp
            | NamedKey::PageDown
            | NamedKey::Insert
            | NamedKey::Delete
            | NamedKey::F1
            | NamedKey::F2
            | NamedKey::F3
            | NamedKey::F4
            | NamedKey::F5
            | NamedKey::F6
            | NamedKey::F7
            | NamedKey::F8
            | NamedKey::F9
            | NamedKey::F10
            | NamedKey::F11
            | NamedKey::F12,
        ) if scan_code != 0 => {
            scan_code = extended_scan(scan_code, modifiers)?;
            0
        }
        _ => return None,
    };
    Some(KeyStroke {
        character,
        scan_code,
    })
}

fn extended_scan(scan: u8, modifiers: ModifiersState) -> Option<u8> {
    if (0x3b..=0x44).contains(&scan) || (0x85..=0x86).contains(&scan) {
        let offset = if modifiers.alt_key() {
            if scan < 0x85 {
                0x2d
            } else {
                6
            }
        } else if modifiers.control_key() {
            if scan < 0x85 {
                0x23
            } else {
                4
            }
        } else if modifiers.shift_key() {
            if scan < 0x85 {
                0x19
            } else {
                2
            }
        } else {
            0
        };
        return Some(scan + offset);
    }
    if modifiers.alt_key() {
        return None;
    }
    if modifiers.control_key() {
        return Some(match scan {
            0x47 => 0x77,
            0x48 => 0x8d,
            0x49 => 0x84,
            0x4b => 0x73,
            0x4d => 0x74,
            0x4f => 0x75,
            0x50 => 0x91,
            0x51 => 0x76,
            0x52 => 0x92,
            0x53 => 0x93,
            _ => return None,
        });
    }
    Some(scan)
}

// PC set-1 positions, used by the existing BIOS translated-key interface.
// Extended keys use BIOS scan codes, not raw E0-prefixed controller bytes.
fn scan_code(key: KeyCode) -> u8 {
    use KeyCode::*;
    match key {
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
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(logical: Key, physical: KeyCode, modifiers: ModifiersState) -> KeyStroke {
        translate(&logical, PhysicalKey::Code(physical), modifiers).unwrap()
    }
    #[test]
    fn translates_characters_control_and_extended_keys() {
        assert_eq!(
            key(
                Key::Character("A".into()),
                KeyCode::KeyA,
                ModifiersState::SHIFT
            ),
            KeyStroke {
                character: b'A',
                scan_code: 0x1e
            }
        );
        assert_eq!(
            key(
                Key::Character("c".into()),
                KeyCode::KeyC,
                ModifiersState::CONTROL
            )
            .character,
            3
        );
        assert_eq!(
            key(
                Key::Named(NamedKey::ArrowUp),
                KeyCode::ArrowUp,
                ModifiersState::empty()
            ),
            KeyStroke {
                character: 0,
                scan_code: 0x48
            }
        );
        assert_eq!(
            key(
                Key::Named(NamedKey::Tab),
                KeyCode::Tab,
                ModifiersState::SHIFT
            ),
            KeyStroke {
                character: 0,
                scan_code: 0x0f
            }
        );
        assert!(translate(
            &Key::Character("é".into()),
            PhysicalKey::Code(KeyCode::KeyE),
            ModifiersState::empty()
        )
        .is_none());
        assert_eq!(
            key(Key::Named(NamedKey::F1), KeyCode::F1, ModifiersState::SHIFT),
            KeyStroke {
                character: 0,
                scan_code: 0x54
            }
        );
        assert_eq!(
            key(
                Key::Named(NamedKey::ArrowLeft),
                KeyCode::ArrowLeft,
                ModifiersState::CONTROL
            ),
            KeyStroke {
                character: 0,
                scan_code: 0x73
            }
        );
        assert_eq!(
            key(
                Key::Character("1".into()),
                KeyCode::Digit1,
                ModifiersState::ALT
            ),
            KeyStroke {
                character: 0,
                scan_code: 0x78
            }
        );
        assert!(translate(
            &Key::Character("1".into()),
            PhysicalKey::Code(KeyCode::Digit1),
            ModifiersState::CONTROL
        )
        .is_none());
    }
}
