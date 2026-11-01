use super::video::{video, PROGRAM};
use crate::common;

#[test]
fn adapter_queries_report_bios_configuration_independently_of_display_mode() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for mode in [3, 7, 0x0f, 0x13] {
            video(&mut dos, mode, 0, 0, 0);
            let before = dos.vga().borrow().snapshot();
            let identity = video(&mut dos, 0x1a00, 0xffff, 0xa5c3, 0x1234);
            assert_eq!(identity.registers.eax as u16, 0x1a1a);
            assert_eq!(identity.registers.ebx as u16, 8); // One color VGA display.
            assert_eq!(identity.registers.ecx as u16, 0xa5c3);
            assert_eq!(identity.registers.edx as u16, 0x1234);
            let config = video(&mut dos, 0x12a5, 0xff10, 0xffff, 0x1234);
            assert_eq!(config.registers.eax as u16, 0x1203);
            assert_eq!(config.registers.ebx as u16, 3); // Color adapter, 256 KiB.
            assert_eq!(config.registers.ecx as u16, 8); // Startup 200-line profile.
            assert_eq!(config.registers.edx as u16, 0x1234);
            let after = dos.vga().borrow().snapshot();
            assert_eq!(after.registers, before.registers);
            for plane in 0..4 {
                assert!(std::ptr::eq(after.plane(plane), before.plane(plane)));
            }
        }
        video(&mut dos, 0x1201, 0x30, 0, 0);
        assert_eq!(video(&mut dos, 0x1200, 0x10, 0, 0).registers.ecx as u16, 9);
        // Read guest-visible BIOS fields, not a second host-side configuration.
        {
            let mut context = dos.cpu().context();
            context.memory.write_u8(0x487, 0xa3).unwrap();
            context.memory.write_u8(0x488, 0xb5).unwrap();
        }
        let config = video(&mut dos, 0x12ff, 0xff10, 0xffff, 0);
        assert_eq!(config.registers.eax as u16, 0x1201);
        assert_eq!(config.registers.ebx as u16, 0x0101);
        assert_eq!(config.registers.ecx as u16, 0x0b05);
    });
}

#[test]
fn display_combination_set_uses_the_bios_table_and_equipment_active_selection() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let before = dos.vga().borrow().snapshot();
        for (codes, index, expected) in [
            (7, 0x0d, 7),
            (0x0700, 0x0d, 7),
            (0, 0, 0),
            (0x0108, 12, 0x0108),
        ] {
            let result = video(&mut dos, 0x1a01, codes, 0xa5c3, 0x1234);
            assert_eq!(result.registers.eax as u16, 0x1a1a);
            assert_eq!(result.registers.ebx as u16, codes);
            assert_eq!(result.registers.ecx as u16, 0xa5c3);
            assert_eq!(result.registers.edx as u16, 0x1234);
            assert_eq!(dos.cpu().context().memory.read_u8(0x48a).unwrap(), index);
            assert_eq!(
                video(&mut dos, 0x1a00, 0, 0, 0).registers.ebx as u16,
                expected
            );
        }
        dos.cpu().context().memory.write_u8(0x410, 0x30).unwrap();
        assert_eq!(
            video(&mut dos, 0x1a00, 0, 0, 0).registers.ebx as u16,
            0x0801
        );
        let after = dos.vga().borrow().snapshot();
        assert_eq!(after.registers, before.registers);
        for plane in 0..4 {
            assert!(std::ptr::eq(after.plane(plane), before.plane(plane)));
        }
        // Mode setup preserves display-combination metadata, including no-clear.
        video(&mut dos, 0x0087, 0, 0, 0);
        assert_eq!(
            video(&mut dos, 0x1a00, 0, 0, 0).registers.ebx as u16,
            0x0801
        );
        video(&mut dos, 0x1a01, 0x0808, 0, 0); // No such pair in IBM's table.
        assert_eq!(dos.cpu().context().memory.read_u8(0x48a).unwrap(), 255);
        assert_eq!(
            video(&mut dos, 0x1a00, 0, 0, 0).registers.ebx as u16,
            0xffff
        );
        // A directly modified invalid table index has the same unknown result.
        dos.cpu().context().memory.write_u8(0x48a, 16).unwrap();
        assert_eq!(
            video(&mut dos, 0x1a00, 0, 0, 0).registers.ebx as u16,
            0xffff
        );
    });
}
