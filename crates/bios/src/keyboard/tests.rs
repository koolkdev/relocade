use super::*;
use relocade_machine::MachineConfig;
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};

pub(super) fn keyboard() -> (Keyboard, Memory) {
    let map =
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap();
    let memory = Memory::new(MachineConfig::new(0x10000, map).unwrap()).unwrap();
    (Keyboard::new(memory.clone()), memory)
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
    assert_eq!(keyboard.try_read_key().unwrap().character, 0);
    assert_eq!(keyboard.write(&bytes[15..]).unwrap(), 1); // Retry only the unwritten suffix.
    assert_eq!(memory.view().read_u16(0x41c).unwrap(), 0x1e);

    // Flushing by editing RAM must not let peek/read resurrect rejected input.
    memory.view().write_u16(0x41a, 0x1e).unwrap();
    for _ in 0..2 {
        assert_eq!(keyboard.peek_key(), None);
        assert_eq!(keyboard.try_read_key(), None);
        assert_eq!(memory.view().read_u16(0x41c).unwrap(), 0x1e);
    }
    keyboard.write_all(&bytes[16..]).unwrap();
    for character in &bytes[16..] {
        assert_eq!(
            keyboard.try_read_key(),
            Some(KeyStroke {
                character: *character,
                scan_code: 0
            })
        );
    }
    assert_eq!(keyboard.peek_key(), None);

    // write_all can accept a prefix before encountering WouldBlock.
    assert_eq!(
        keyboard.write_all(&[b'X'; 16]).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    for _ in 0..15 {
        assert_eq!(keyboard.try_read_key().unwrap().character, b'X');
    }
    assert_eq!(keyboard.try_read_key(), None);
    keyboard.write_all(b"clear me").unwrap();
    keyboard.discard_queued();
    assert_eq!(keyboard.peek_key(), None);
}
