use super::*;
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
    let mut keyboard = PhysicalKeyboard::default();
    for (code, text, character, scan_code) in [
        (KeyCode::KeyQ, Some("A"), b'q', 0x10),
        (KeyCode::KeyA, Some("ש"), b'a', 0x1e),
        (KeyCode::KeyE, Some("é"), b'e', 0x12),
        (KeyCode::KeyE, Some("e\u{301}"), b'e', 0x12),
        (KeyCode::Quote, None, b'\'', 0x28),
        (KeyCode::ArrowUp, Some("8"), 0xe0, 0x48),
        (KeyCode::Enter, None, b'\r', 0x1c),
    ] {
        assert_eq!(
            keyboard.translate(press(code, text)),
            Some(KeyStroke {
                character,
                scan_code
            })
        );
    }
    assert_eq!(
        keyboard.translate(press(KeyCode::MediaPlayPause, None)),
        None
    );
    let mut repeat = press(KeyCode::KeyE, Some("é"));
    if let KeyboardEvent::Key { repeat, .. } = &mut repeat {
        *repeat = true;
    }
    assert_eq!(keyboard.translate(repeat).unwrap().character, b'e');
}

#[test]
fn physical_modifiers_select_guest_characters_and_focus_releases_them() {
    let mut keyboard = PhysicalKeyboard::default();
    keyboard.translate(press(KeyCode::ControlRight, None));
    assert_eq!(
        keyboard.translate(press(KeyCode::KeyQ, Some("A"))),
        Some(KeyStroke {
            character: 17,
            scan_code: 0x10
        })
    );
    keyboard.translate(release(KeyCode::ControlRight));
    keyboard.translate(press(KeyCode::ShiftRight, None));
    assert_eq!(
        keyboard
            .translate(press(KeyCode::KeyA, Some("ש")))
            .unwrap()
            .character,
        b'A'
    );
    assert_eq!(
        keyboard
            .translate(press(KeyCode::Digit2, Some("2")))
            .unwrap()
            .character,
        b'@'
    );
    keyboard.translate(press(KeyCode::CapsLock, None));
    // Duplicate presses cannot toggle locks again, even with a missing repeat flag.
    keyboard.translate(press(KeyCode::CapsLock, None));
    assert_eq!(
        keyboard
            .translate(press(KeyCode::KeyA, Some("A")))
            .unwrap()
            .character,
        b'a'
    );
    keyboard.translate(KeyboardEvent::FocusLost);
    assert_eq!(
        keyboard.shift_state().bits(),
        0x40,
        "focus loss preserves lock state"
    );
    assert_eq!(
        keyboard
            .translate(press(KeyCode::KeyA, Some("a")))
            .unwrap()
            .character,
        b'A'
    );
}

#[test]
fn modifier_snapshots_restore_keys_held_before_attachment_or_focus() {
    let mut keyboard = PhysicalKeyboard::default();
    for (modifiers, code, character, flags) in [
        (ModifiersState::SHIFT, KeyCode::KeyA, b'A', 2),
        (ModifiersState::CONTROL, KeyCode::KeyC, 3, 4),
        (ModifiersState::ALT, KeyCode::KeyA, 0, 8),
        (ModifiersState::empty(), KeyCode::KeyA, b'a', 0),
    ] {
        assert_eq!(
            keyboard.translate(KeyboardEvent::Modifiers(modifiers)),
            None
        );
        assert_eq!(keyboard.shift_state().bits(), flags);
        assert_eq!(
            keyboard.translate(press(code, None)).unwrap().character,
            character
        );
    }
    keyboard.translate(press(KeyCode::ShiftRight, None));
    keyboard.translate(KeyboardEvent::Modifiers(ModifiersState::SHIFT));
    assert_eq!(
        keyboard.shift_state().bits(),
        1,
        "preserve the known modifier side"
    );
}

#[test]
fn keypad_uses_guest_num_lock_with_shift_override() {
    let mut keyboard = PhysicalKeyboard::default();
    assert_eq!(
        keyboard.translate(press(KeyCode::Numpad8, Some("8"))),
        Some(KeyStroke {
            character: 0,
            scan_code: 0x48
        })
    );
    keyboard.translate(press(KeyCode::NumLock, None));
    assert_eq!(
        keyboard
            .translate(press(KeyCode::Numpad8, None))
            .unwrap()
            .character,
        b'8'
    );
    keyboard.translate(press(KeyCode::ShiftLeft, None));
    assert_eq!(
        keyboard
            .translate(press(KeyCode::Numpad8, Some("8")))
            .unwrap()
            .character,
        0
    );
    assert_eq!(
        keyboard
            .translate(press(KeyCode::NumpadDivide, None))
            .unwrap()
            .character,
        b'/'
    );
    assert_eq!(keyboard.translate(release(KeyCode::Numpad8)), None);
}

#[test]
fn physical_special_keys_use_bios_modifier_codes() {
    let mut keyboard = PhysicalKeyboard::default();
    for (modifiers, code, character, scan_code) in [
        (ModifiersState::SHIFT, KeyCode::F1, 0, 0x54),
        (ModifiersState::CONTROL, KeyCode::ArrowLeft, 0xe0, 0x73),
        (ModifiersState::ALT, KeyCode::Digit1, 0, 0x78),
        (ModifiersState::ALT, KeyCode::Enter, 0, 0x1c),
        (ModifiersState::CONTROL, KeyCode::NumpadDivide, 0, 0x95),
    ] {
        keyboard.translate(KeyboardEvent::Modifiers(modifiers));
        assert_eq!(
            keyboard.translate(press(code, None)),
            Some(KeyStroke {
                character,
                scan_code
            })
        );
    }
}
