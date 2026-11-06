use super::*;

fn key(word: u16) -> KeyStroke {
    let [character, scan_code] = word.to_le_bytes();
    KeyStroke {
        character,
        scan_code,
    }
}

#[test]
fn legacy_status_discards_only_incompatible_words_and_preserves_the_stored_encoding() {
    for (word, legacy, enhanced) in [
        (0x48e0, Some(0x4800), 0x48e0), // Dedicated Up.
        (0x4800, Some(0x4800), 0x4800), // Keypad Up.
        (0xe00d, Some(0x1c0d), 0xe00d), // Keypad Enter precedes scan-range filtering.
        (0xe00a, Some(0x1c0a), 0xe00a), // Ctrl+keypad Enter.
        (0xe02f, Some(0x352f), 0xe02f), // Keypad divide.
        (0x84e0, Some(0x8400), 0x84e0), // Ctrl+PageUp is still compatible.
        (0x8500, None, 0x8500),         // F11.
        (0x8de0, None, 0x8de0),         // Ctrl+Up.
        (0x1cf0, None, 0x1c00),         // Alt+Enter.
        (0x4cf0, None, 0x4c00),         // Keypad 5 with NumLock off.
        (0x00e0, Some(0x00e0), 0x00e0), // Literal OEM characters.
        (0x00f0, Some(0x00f0), 0x00f0),
    ] {
        let (keyboard, memory) = keyboard();
        assert!(keyboard.store_key(key(word)));
        assert_eq!(keyboard.peek_key(KeyMode::Enhanced), Some(key(enhanced)));
        assert_eq!(keyboard.peek_key(KeyMode::Legacy), legacy.map(key));
        assert_eq!(memory.view().read_u16(0x41e).unwrap(), word);
        if legacy.is_some() {
            assert_eq!(memory.view().read_u16(0x41a).unwrap(), 0x1e);
            assert_eq!(
                keyboard.try_read_key(KeyMode::Enhanced),
                Some(key(enhanced))
            );
        } else {
            assert_eq!(memory.view().read_u16(0x41a).unwrap(), 0x20);
        }
        assert_eq!(keyboard.peek_key(KeyMode::Enhanced), None);
    }
}

#[test]
fn scan_translation_keeps_enhanced_markers_in_guest_ram() {
    for (bytes, word) in [
        (&[0xe0, 0x48][..], 0x48e0),
        (&[0x48][..], 0x4800),
        (&[0xe0, 0x1c][..], 0xe00d),
        (&[0x1d, 0xe0, 0x1c][..], 0xe00a),
        (&[0xe0, 0x35][..], 0xe02f),
        (&[0x57][..], 0x8500),
        (&[0x38, 0x1c][..], 0x1cf0),
        (&[0x38, 0x0f][..], 0xa500),
        (&[0x38, 0xe0, 0x48][..], 0x9800),
        (&[0x4c][..], 0x4cf0),
    ] {
        let (keyboard, memory) = keyboard();
        scans(&keyboard, bytes);
        assert_eq!(
            memory.view().read_u16(0x41e).unwrap(),
            word,
            "scan bytes {bytes:x?}"
        );
    }
}

#[test]
fn a_legacy_read_waits_after_discarding_enhanced_only_input() {
    let (keyboard, _) = keyboard();
    assert!(keyboard.store_key(key(0x8500)));
    let mut read = std::pin::pin!(keyboard.read_key(KeyMode::Legacy));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(read.as_mut().poll(&mut cx).is_pending());
    assert!(keyboard.store_key(key(0x1cf0)));
    assert!(read.as_mut().poll(&mut cx).is_pending());
    assert!(keyboard.store_key(key(0x1e61)));
    assert_eq!(read.as_mut().poll(&mut cx), Poll::Ready(key(0x1e61)));
}
