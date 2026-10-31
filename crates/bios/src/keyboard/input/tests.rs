use super::*;
use relocade_frontend::keyboard::{Key, KeyLocation, NativeKey};

fn press(code: KeyCode, text: Option<&str>) -> KeyboardEvent {
    KeyboardEvent::Key {
        physical_key: code.into(),
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        location: KeyLocation::Standard,
        text: text.map(str::to_owned),
        state: ElementState::Pressed,
        repeat: false,
    }
}

#[test]
fn text_uses_layout_and_scan_codes_use_physical_keys() {
    let keyboard = Keyboard::default();
    let mut translation = Translation::default();
    translation.apply(&keyboard, press(KeyCode::KeyQ, Some("A")));
    assert_eq!(keyboard.try_read().unwrap().word(), 0x1041);
    translation.apply(&keyboard, press(KeyCode::ArrowUp, None));
    assert_eq!(keyboard.try_read_character(), Some(0));
    assert_eq!(keyboard.try_read_character(), Some(0x48));
    translation.apply(&keyboard, press(KeyCode::Numpad8, None));
    assert_eq!(keyboard.try_read().unwrap().word(), 0x4800);
    translation.apply(&keyboard, press(KeyCode::Numpad8, Some("8")));
    assert_eq!(keyboard.try_read().unwrap().word(), 0x4838);
    translation.apply(&keyboard, press(KeyCode::Enter, None));
    assert_eq!(keyboard.try_read().unwrap().word(), 0x1c0d);
    translation.apply(&keyboard, press(KeyCode::KeyE, Some("é")));
    translation.apply(&keyboard, press(KeyCode::MediaPlayPause, None));
    assert!(
        !keyboard.has_input(),
        "unsupported text must not become an extended key"
    );
}

#[test]
fn modifiers_releases_and_focus_do_not_enqueue_characters() {
    let keyboard = Keyboard::default();
    let mut translation = Translation::default();
    translation.apply(&keyboard, KeyboardEvent::Modifiers(ModifiersState::CONTROL));
    translation.apply(&keyboard, press(KeyCode::KeyC, Some("c")));
    assert_eq!(keyboard.try_read().unwrap().word(), 0x2e03);
    translation.apply(&keyboard, press(KeyCode::KeyQ, Some("A")));
    assert_eq!(keyboard.try_read().unwrap().word(), 0x1001);
    translation.apply(&keyboard, press(KeyCode::CapsLock, None));
    translation.apply(
        &keyboard,
        KeyboardEvent::Key {
            physical_key: KeyCode::CapsLock.into(),
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            location: KeyLocation::Standard,
            text: None,
            state: ElementState::Pressed,
            repeat: true,
        },
    );
    translation.apply(
        &keyboard,
        KeyboardEvent::Key {
            physical_key: KeyCode::KeyC.into(),
            logical_key: Key::Character("c".into()),
            location: KeyLocation::Standard,
            text: Some("c".into()),
            state: ElementState::Released,
            repeat: false,
        },
    );
    translation.apply(&keyboard, KeyboardEvent::FocusLost);
    assert!(!keyboard.has_input());
    assert_eq!(
        keyboard.0.borrow().shifts.bits(),
        0x40,
        "focus loss preserves toggle state"
    );
}
