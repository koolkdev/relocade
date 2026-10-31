use super::*;

fn press(code: KeyCode, text: Option<&str>) -> KeyboardEvent {
    KeyboardEvent::Key {
        code,
        text: text.map(str::to_owned),
        pressed: true,
        repeat: false,
    }
}

#[test]
fn text_uses_host_layout_and_scan_codes_use_physical_keys() {
    let keyboard = Keyboard::default();
    let mut host = Translation::default();
    host.apply(&keyboard, press(KeyCode::KeyQ, Some("A")));
    assert_eq!(keyboard.try_read().unwrap().word(), 0x1041);
    host.apply(&keyboard, press(KeyCode::ArrowUp, None));
    assert_eq!(keyboard.try_read_character(), Some(0));
    assert_eq!(keyboard.try_read_character(), Some(0x48));
    host.apply(&keyboard, press(KeyCode::Numpad8, None));
    assert_eq!(keyboard.try_read().unwrap().word(), 0x4800);
    host.apply(&keyboard, press(KeyCode::Numpad8, Some("8")));
    assert_eq!(keyboard.try_read().unwrap().word(), 0x4838);
    host.apply(&keyboard, press(KeyCode::Enter, None));
    assert_eq!(keyboard.try_read().unwrap().word(), 0x1c0d);
    host.apply(&keyboard, press(KeyCode::KeyE, Some("é")));
    assert!(
        !keyboard.has_input(),
        "unsupported text must not become an extended key"
    );
}

#[test]
fn modifiers_releases_and_focus_do_not_enqueue_characters() {
    let keyboard = Keyboard::default();
    let mut host = Translation::default();
    host.apply(
        &keyboard,
        KeyboardEvent::Modifiers(Modifiers {
            control: true,
            ..Default::default()
        }),
    );
    host.apply(&keyboard, press(KeyCode::KeyC, Some("c")));
    assert_eq!(keyboard.try_read().unwrap().word(), 0x2e03);
    host.apply(&keyboard, press(KeyCode::KeyQ, Some("A")));
    assert_eq!(keyboard.try_read().unwrap().word(), 0x1001);
    host.apply(&keyboard, press(KeyCode::CapsLock, None));
    host.apply(
        &keyboard,
        KeyboardEvent::Key {
            code: KeyCode::CapsLock,
            text: None,
            pressed: true,
            repeat: true,
        },
    );
    host.apply(
        &keyboard,
        KeyboardEvent::Key {
            code: KeyCode::KeyC,
            text: Some("c".into()),
            pressed: false,
            repeat: false,
        },
    );
    host.apply(&keyboard, KeyboardEvent::FocusLost);
    assert!(!keyboard.has_input());
    assert_eq!(
        keyboard.0.borrow().shifts.bits(),
        0x40,
        "focus loss preserves toggle state"
    );
}
