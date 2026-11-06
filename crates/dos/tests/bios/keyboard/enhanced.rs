use super::{common, invoke, PROGRAM};

#[test]
fn enhanced_services_and_legacy_filtering_share_the_guest_ring() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let memory = dos.cpu().memory();
        for word in [0x8500, 0x48e0, 0xe00d, 0x00e0, 0x00f0, 0x1cf0] {
            dos.cpu().cpu_state().set_cx(word);
            assert_eq!(invoke(&mut dos, 0x100, 0x0500).registers.eax as u16, 0x0500);
        }
        assert_eq!(invoke(&mut dos, 0x100, 0x1100).registers.eax as u16, 0x8500);
        assert_eq!(memory.view().read_u16(0x41a).unwrap(), 0x1e);
        // Legacy peek drops F11 but leaves the Up key's E0 marker in RAM.
        assert_eq!(invoke(&mut dos, 0x100, 0x0100).registers.eax as u16, 0x4800);
        assert_eq!(memory.view().read_u16(0x41a).unwrap(), 0x20);
        assert_eq!(memory.view().read_u16(0x420).unwrap(), 0x48e0);
        for expected in [0x48e0, 0xe00d, 0x00e0, 0x00f0, 0x1c00] {
            let peek = invoke(&mut dos, 0x100, 0x1100);
            assert_eq!(peek.registers.eax as u16, expected);
            assert_eq!(peek.flags.eflags(), 0x603);
            let read = invoke(&mut dos, 0x100, 0x1000);
            assert_eq!(read.registers.eax as u16, expected);
            assert_eq!(read.flags.eflags(), 0x643);
        }
        let empty = invoke(&mut dos, 0x100, 0x117b);
        assert_eq!(empty.registers.eax as u16, 0x117b);
        assert_eq!(empty.flags.eflags(), 0x643);
    });
}

#[test]
fn enhanced_shift_status_maps_left_right_and_system_flags_from_guest_ram() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let memory = dos.cpu().memory();
        for (held, status, expected) in [
            (3, 0x1c, 0x0fa5),
            (0x77, 0x1c, 0xffa5),
            (0x88, 0xf3, 0x00a5),
            (4, 0x10, 0x80a5),
        ] {
            memory.view().write_u8(0x417, 0xa5).unwrap();
            memory.view().write_u8(0x418, held).unwrap();
            memory.view().write_u8(0x496, status).unwrap();
            let result = invoke(&mut dos, 0x100, 0x1200);
            assert_eq!(result.registers.eax as u16, expected);
            assert_eq!(result.flags.eflags(), 0x643);
            assert_eq!(memory.view().read_u8(0x418).unwrap(), held);
            assert_eq!(memory.view().read_u8(0x496).unwrap(), status);
        }
    });
}

#[test]
fn dos_reads_enhanced_keys_as_bytes_without_losing_literal_oem_characters() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for word in [0x48e0, 0x8500, 0x00e0, 0x00f0, 0xe00d, 0x1cf0] {
            dos.cpu().cpu_state().set_cx(word);
            invoke(&mut dos, 0x100, 0x0500);
        }
        for byte in [0, 0x48, 0, 0x85, 0xe0, 0xf0, b'\r', 0, 0x1c] {
            assert_eq!(invoke(&mut dos, 0x104, 0x0700).registers.eax as u8, byte);
        }
        assert_ne!(invoke(&mut dos, 0x100, 0x1100).flags.eflags() & 0x40, 0);
    });
}
