use super::*;
use relocade_machine::MachineConfig;
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};

mod enhanced;

fn keyboard() -> (Keyboard, Memory) {
    let map =
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap();
    let memory = Memory::new(MachineConfig::new(0x10000, map).unwrap()).unwrap();
    (Keyboard::new(memory.clone()), memory)
}

fn scans(keyboard: &Keyboard, bytes: &[u8]) {
    for &byte in bytes {
        keyboard.scan_code(byte);
    }
}

#[test]
fn guest_ring_and_shift_edits_are_authoritative() {
    let (keyboard, memory) = keyboard();
    scans(&keyboard, &[0x1e, 0x9e]);
    assert_eq!(memory.view().read_u16(0x41a).unwrap(), 0x1e);
    assert_eq!(memory.view().read_u16(0x41c).unwrap(), 0x20);
    assert_eq!(memory.view().read_u16(0x41e).unwrap(), 0x1e61);
    memory.view().write_u16(0x41a, 0x20).unwrap();
    assert_eq!(
        keyboard.peek_key(KeyMode::Enhanced),
        None,
        "guest can flush the buffer"
    );
    memory.view().write_u16(0x420, 0x2c5a).unwrap();
    memory.view().write_u16(0x41c, 0x22).unwrap();
    assert_eq!(
        keyboard.try_read_key(KeyMode::Enhanced),
        Some(KeyStroke {
            character: b'Z',
            scan_code: 0x2c
        })
    );
    memory.view().write_u8(0x417, 0x40).unwrap();
    scans(&keyboard, &[0x1e]);
    assert_eq!(
        keyboard.try_read_key(KeyMode::Enhanced).unwrap().character,
        b'A'
    );
    assert!(keyboard.shift_state().caps_lock);
}

#[test]
fn physical_input_drops_on_full_ring_then_wraps_and_honors_relocation() {
    let (keyboard, memory) = keyboard();
    for _ in 0..16 {
        scans(&keyboard, &[0x1e, 0x9e]);
    }
    assert_eq!(memory.view().read_u16(0x41c).unwrap(), 0x3c);
    assert_eq!(
        keyboard.try_read_key(KeyMode::Enhanced).unwrap().character,
        b'a'
    );
    scans(&keyboard, &[0x30, 0xb0]);
    assert_eq!(memory.view().read_u16(0x41c).unwrap(), 0x1e);
    for _ in 0..14 {
        assert_eq!(
            keyboard.try_read_key(KeyMode::Enhanced).unwrap().character,
            b'a'
        );
    }
    assert_eq!(
        keyboard.try_read_key(KeyMode::Enhanced).unwrap().character,
        b'b'
    );
    assert_eq!(keyboard.try_read_key(KeyMode::Enhanced), None);
    for (address, value) in [
        (0x480, 0x200),
        (0x482, 0x206),
        (0x41a, 0x204),
        (0x41c, 0x204),
    ] {
        memory.view().write_u16(address, value).unwrap();
    }
    scans(&keyboard, &[0x2e]);
    assert_eq!(memory.view().read_u16(0x604).unwrap(), 0x2e63);
    assert_eq!(memory.view().read_u16(0x41c).unwrap(), 0x200);
    assert_eq!(
        keyboard.try_read_key(KeyMode::Enhanced).unwrap().character,
        b'c'
    );
    assert_eq!(memory.view().read_u16(0x41a).unwrap(), 0x200);
}

#[test]
fn modifier_sides_locks_and_special_sequences_update_bda_without_phantom_keys() {
    let (keyboard, memory) = keyboard();
    scans(&keyboard, &[0x1d, 0xe0, 0x1d, 0x9d]);
    assert!(keyboard.shift_state().control, "right Ctrl remains down");
    assert_eq!(memory.view().read_u8(0x418).unwrap() & 3, 0);
    assert_eq!(memory.view().read_u8(0x496).unwrap() & 0x0c, 4);
    scans(&keyboard, &[0x10]);
    assert_eq!(
        keyboard.try_read_key(KeyMode::Enhanced).unwrap().character,
        17
    );
    scans(&keyboard, &[0xe0, 0x9d, 0x36, 0x3a, 0x3a, 0x1e]);
    assert_eq!(
        keyboard.try_read_key(KeyMode::Enhanced).unwrap().character,
        b'a'
    );
    assert_eq!(keyboard.shift_state().bits(), 0x41);
    scans(&keyboard, &[0xb6, 0xba]);
    assert_eq!(
        keyboard.shift_state().bits(),
        0x40,
        "break preserves the lock toggle"
    );
    scans(&keyboard, &[0xe0, 0x2a, 0xe0, 0x37, 0xe0, 0xb7, 0xe0, 0xaa]);
    scans(&keyboard, &[0xe1, 0x1d, 0x45, 0xe1, 0x9d, 0xc5]);
    assert_eq!(keyboard.shift_state().bits(), 0x40);
    assert_eq!(keyboard.peek_key(KeyMode::Enhanced), None);
    assert_eq!(memory.view().read_u8(0x496).unwrap() & 3, 0);
}

#[test]
fn us_layout_distinguishes_keypad_navigation_and_shifted_punctuation() {
    let (keyboard, _) = keyboard();
    for (bytes, character, scan_code) in [
        (&[0x12][..], b'e', 0x12),
        (&[0x28][..], b'\'', 0x28),
        (&[0x2a, 0x03][..], b'@', 3),
        (&[0xb6, 0xaa, 0x45, 0xc5, 0x48][..], b'8', 0x48),
        (&[0xe0, 0x48][..], 0xe0, 0x48),
        (&[0x2a, 0x48][..], 0, 0x48),
        (&[0xe0, 0x35][..], b'/', 0xe0),
        (&[0xaa, 0xe0, 0x1c][..], b'\r', 0xe0),
        (&[0x57][..], 0, 0x85),
        (&[0x2a, 0x3b][..], 0, 0x54),
        (&[0xaa, 0x38, 0x3b][..], 0, 0x68),
    ] {
        scans(&keyboard, bytes);
        assert_eq!(
            keyboard.try_read_key(KeyMode::Enhanced),
            Some(KeyStroke {
                character,
                scan_code
            })
        );
        assert_eq!(keyboard.peek_key(KeyMode::Enhanced), None);
    }
}

#[test]
fn buffer_writes_report_capacity_and_guest_flush_leaves_no_hidden_input() {
    let (mut keyboard, memory) = keyboard();
    let bytes: Vec<u8> = (0..20).collect();
    assert_eq!(keyboard.write(&bytes).unwrap(), 15);
    assert_eq!(memory.view().read_u16(0x41c).unwrap(), 0x3c);
    assert_eq!(
        keyboard.write(&bytes[15..]).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(keyboard.write(&[]).unwrap(), 0);
    assert!(!keyboard.store_key(KeyStroke {
        character: b'X',
        scan_code: 0x2d
    }));
    assert_eq!(memory.view().read_u16(0x41c).unwrap(), 0x3c);
    assert_eq!(
        keyboard.try_read_key(KeyMode::Enhanced).unwrap().character,
        0
    );
    assert_eq!(keyboard.write(&bytes[15..]).unwrap(), 1); // Retry only the unwritten suffix.
    assert_eq!(memory.view().read_u16(0x41c).unwrap(), 0x1e);

    // Flushing by editing RAM must not let peek/read resurrect rejected input.
    memory.view().write_u16(0x41a, 0x1e).unwrap();
    for _ in 0..2 {
        assert_eq!(keyboard.peek_key(KeyMode::Enhanced), None);
        assert_eq!(keyboard.try_read_key(KeyMode::Enhanced), None);
        assert_eq!(memory.view().read_u16(0x41c).unwrap(), 0x1e);
    }
    keyboard.write_all(&bytes[16..]).unwrap();
    for character in &bytes[16..] {
        assert_eq!(
            keyboard.try_read_key(KeyMode::Enhanced),
            Some(KeyStroke {
                character: *character,
                scan_code: 0
            })
        );
    }
    assert_eq!(keyboard.peek_key(KeyMode::Enhanced), None);

    // write_all can accept a prefix before encountering WouldBlock.
    assert_eq!(
        keyboard.write_all(&[b'X'; 16]).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    for _ in 0..15 {
        assert_eq!(
            keyboard.try_read_key(KeyMode::Enhanced).unwrap().character,
            b'X'
        );
    }
    assert_eq!(keyboard.try_read_key(KeyMode::Enhanced), None);
    keyboard.write_all(b"clear me").unwrap();
    keyboard.discard_queued();
    assert_eq!(keyboard.peek_key(KeyMode::Enhanced), None);
}

#[test]
fn control_and_alt_select_bios_codes_instead_of_plain_characters() {
    for (bytes, character, scan_code) in [
        (&[0x1d, 0xe0, 0x47][..], 0xe0, 0x77),    // Ctrl+Home.
        (&[0x1d, 0x45, 0xc5, 0x47][..], 0, 0x77), // Ctrl overrides NumLock.
        (&[0x1d, 0x0f][..], 0, 0x94),             // Ctrl+Tab.
        (&[0x1d, 0xe0, 0x35][..], 0, 0x95),       // Ctrl+keypad divide.
        (&[0x1d, 0x0e][..], 127, 0x0e),           // Ctrl+Backspace.
        (&[0x1d, 0x39][..], b' ', 0x39),          // IBM's Ctrl+Space entry is space.
        (&[0x38, 0x39][..], b' ', 0x39),          // Alt+Space.
        (&[0x38, 0xe0, 0x47][..], 0, 0x97),       // Alt+Home.
        (&[0x38, 0x03][..], 0, 0x79),             // Alt+2.
        (&[0x2a, 0x1d, 0x38, 0x58][..], 0, 0x8c), // Alt wins over Ctrl/Shift.
    ] {
        let (keyboard, _) = keyboard();
        scans(&keyboard, bytes);
        assert_eq!(
            keyboard.try_read_key(KeyMode::Enhanced),
            Some(KeyStroke {
                character,
                scan_code
            })
        );
        assert_eq!(keyboard.peek_key(KeyMode::Enhanced), None);
    }
    for bytes in [&[0x1d, 0x02][..], &[0x1d, 0x35], &[0x38, 0x47]] {
        let (keyboard, _) = keyboard();
        scans(&keyboard, bytes);
        assert_eq!(
            keyboard.peek_key(KeyMode::Enhanced),
            None,
            "unsupported combination {bytes:x?}"
        );
    }
}

#[test]
fn scan_updates_preserve_guest_flag_bits_and_use_guest_shift_state() {
    let (keyboard, memory) = keyboard();
    memory.view().write_u8(0x417, 4).unwrap(); // Guest sets Ctrl without a side flag.
    memory.view().write_u8(0x418, 0x0c).unwrap(); // SysRq and hold state.
    memory.view().write_u8(0x496, 0xf0).unwrap(); // Enhanced keyboard and other BIOS bits.
    scans(&keyboard, &[0x1e]);
    assert_eq!(
        keyboard.try_read_key(KeyMode::Enhanced),
        Some(KeyStroke {
            character: 1,
            scan_code: 0x1e
        })
    );
    scans(&keyboard, &[0x2a, 0xe0, 0x38]); // Left Shift and right Alt.
    assert_eq!(memory.view().read_u8(0x417).unwrap(), 0x0e);
    assert_eq!(memory.view().read_u8(0x418).unwrap(), 0x0c);
    assert_eq!(memory.view().read_u8(0x496).unwrap(), 0xf8);
    scans(&keyboard, &[0xaa, 0xe0, 0xb8]);
    assert_eq!(memory.view().read_u8(0x417).unwrap(), 4);
    assert_eq!(memory.view().read_u8(0x418).unwrap(), 0x0c);
    assert_eq!(memory.view().read_u8(0x496).unwrap(), 0xf0);
}
