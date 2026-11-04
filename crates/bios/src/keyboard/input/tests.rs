use super::*;
use crate::KeyStroke;
use relocade_frontend::keyboard::{Key, KeyLocation, ModifiersState, NativeKey};

fn key(code: KeyCode, text: Option<&str>, state: ElementState) -> KeyboardEvent {
    KeyboardEvent::Key {
        physical_key: code.into(),
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        location: KeyLocation::Standard,
        text: text.map(str::to_owned),
        state,
        repeat: false,
    }
}

fn press(code: KeyCode, text: Option<&str>) -> KeyboardEvent {
    key(code, text, ElementState::Pressed)
}

fn release(code: KeyCode) -> KeyboardEvent {
    key(code, None, ElementState::Released)
}

#[test]
fn physical_keys_ignore_host_language_text_and_composition() {
    let keyboard = Keyboard::default();
    let mut translation = Translation::default();
    for (code, text, character, scan_code) in [
        (KeyCode::KeyQ, Some("A"), b'q', 0x10),
        (KeyCode::KeyA, Some("ש"), b'a', 0x1e),
        (KeyCode::KeyE, Some("é"), b'e', 0x12),
        (KeyCode::KeyE, Some("e\u{301}"), b'e', 0x12),
        (KeyCode::Quote, None, b'\'', 0x28),
        (KeyCode::ArrowUp, Some("8"), 0, 0x48),
        (KeyCode::Enter, None, b'\r', 0x1c),
    ] {
        translation.apply(&keyboard, press(code, text));
        assert_eq!(
            keyboard.try_read_key(),
            Some(KeyStroke {
                character,
                scan_code
            })
        );
        assert_eq!(keyboard.peek_key(), None, "one key produces one BIOS word");
    }
    translation.apply(&keyboard, press(KeyCode::MediaPlayPause, None));
    assert_eq!(keyboard.peek_key(), None);
}

#[test]
fn physical_modifiers_select_guest_characters_and_focus_releases_them() {
    let keyboard = Keyboard::default();
    let mut translation = Translation::default();
    translation.apply(&keyboard, press(KeyCode::ControlRight, None));
    translation.apply(&keyboard, press(KeyCode::KeyQ, Some("A")));
    assert_eq!(
        keyboard.try_read_key(),
        Some(KeyStroke {
            character: 17,
            scan_code: 0x10
        })
    );
    translation.apply(&keyboard, release(KeyCode::ControlRight));
    translation.apply(&keyboard, press(KeyCode::ShiftRight, None));
    translation.apply(&keyboard, press(KeyCode::KeyA, Some("ש")));
    assert_eq!(keyboard.try_read_key().unwrap().character, b'A');
    translation.apply(&keyboard, press(KeyCode::Digit2, Some("2")));
    assert_eq!(keyboard.try_read_key().unwrap().character, b'@');
    translation.apply(&keyboard, press(KeyCode::CapsLock, None));
    // Duplicate presses cannot toggle locks again, even with a missing repeat flag.
    translation.apply(&keyboard, press(KeyCode::CapsLock, None));
    translation.apply(&keyboard, press(KeyCode::KeyA, Some("A")));
    assert_eq!(keyboard.try_read_key().unwrap().character, b'a');
    translation.apply(&keyboard, KeyboardEvent::FocusLost);
    assert_eq!(
        keyboard.shift_state().bits(),
        0x40,
        "focus loss preserves lock state"
    );
    translation.apply(&keyboard, press(KeyCode::KeyA, Some("a")));
    assert_eq!(keyboard.try_read_key().unwrap().character, b'A');
    assert_eq!(keyboard.peek_key(), None);
}

#[test]
fn modifier_snapshots_restore_keys_held_before_attachment_or_focus() {
    let keyboard = Keyboard::default();
    let mut translation = Translation::default();
    for (modifiers, code, character, flags) in [
        (ModifiersState::SHIFT, KeyCode::KeyA, b'A', 2),
        (ModifiersState::CONTROL, KeyCode::KeyC, 3, 4),
        (ModifiersState::ALT, KeyCode::KeyA, 0, 8),
        (ModifiersState::empty(), KeyCode::KeyA, b'a', 0),
    ] {
        translation.apply(&keyboard, KeyboardEvent::Modifiers(modifiers));
        assert_eq!(keyboard.shift_state().bits(), flags);
        assert_eq!(keyboard.peek_key(), None);
        translation.apply(&keyboard, press(code, None));
        assert_eq!(keyboard.try_read_key().unwrap().character, character);
    }
    translation.apply(&keyboard, press(KeyCode::ShiftRight, None));
    translation.apply(&keyboard, KeyboardEvent::Modifiers(ModifiersState::SHIFT));
    assert_eq!(
        keyboard.shift_state().bits(),
        1,
        "preserve the known modifier side"
    );
}

#[test]
fn keypad_uses_guest_num_lock_with_shift_override() {
    let keyboard = Keyboard::default();
    let mut translation = Translation::default();
    translation.apply(&keyboard, press(KeyCode::Numpad8, Some("8")));
    assert_eq!(
        keyboard.try_read_key(),
        Some(KeyStroke {
            character: 0,
            scan_code: 0x48
        })
    );
    translation.apply(&keyboard, press(KeyCode::NumLock, None));
    translation.apply(&keyboard, press(KeyCode::Numpad8, None));
    assert_eq!(keyboard.try_read_key().unwrap().character, b'8');
    translation.apply(&keyboard, press(KeyCode::ShiftLeft, None));
    translation.apply(&keyboard, press(KeyCode::Numpad8, Some("8")));
    assert_eq!(keyboard.try_read_key().unwrap().character, 0);
    translation.apply(&keyboard, press(KeyCode::NumpadDivide, None));
    assert_eq!(keyboard.try_read_key().unwrap().character, b'/');
    translation.apply(&keyboard, release(KeyCode::Numpad8));
    assert_eq!(keyboard.peek_key(), None);
}
