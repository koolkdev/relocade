use super::{video, PROGRAM};
use crate::common;

#[test]
fn video_addressing_gates_cpu_access_and_preserves_display_and_bios_state() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (mode, address, status_port) in [
            (3, 0xb8000, 0x3da),
            (7, 0xb0000, 0x3ba),
            (0x13, 0xa0000, 0x3da),
        ] {
            video(&mut dos, mode, 0, 0, 0);
            let mut bios_state = [0; 0x42];
            {
                let mut context = dos.cpu().context();
                context.memory.write_u16(address, 0x0741).unwrap();
                context.memory.read(0x449, &mut bios_state).unwrap();
                // The service must not disturb either streaming register phase.
                context.io.read_port(status_port, 1);
                context.io.write_port(0x3c0, 1, 0x30);
                context.io.write_port(0x3c8, 1, 9);
                context.io.write_port(0x3c9, 1, 12);
            }
            let before = dos.vga().borrow().snapshot();
            let before_registers = dos.vga().borrow().registers().clone();
            for (selection, expected_ax, accessible) in [
                (1, 0x1212, false),
                (2, 0x1200, false),
                (255, 0x1200, false),
                (0, 0x1212, true),
                (2, 0x1200, true),
            ] {
                let result = video(&mut dos, 0x1200 | selection, 0xa532, 0x5678, 0x9abc);
                assert_eq!(result.registers.eax as u16, expected_ax);
                assert_eq!(result.registers.ebx as u16, 0xa532);
                assert_eq!(result.registers.ecx as u16, 0x5678);
                assert_eq!(result.registers.edx as u16, 0x9abc);
                {
                    let mut context = dos.cpu().context();
                    assert_eq!(
                        context.memory.read_u16(address).unwrap(),
                        if accessible { 0x0741 } else { 0xffff }
                    );
                    assert_eq!(
                        context.io.read_port(0x3cc, 1),
                        if accessible {
                            u32::from(before.vga().unwrap().miscellaneous)
                        } else {
                            0xff
                        }
                    );
                    if !accessible {
                        context.memory.write_u16(address, 0).unwrap();
                        context.io.write_port(0x3c4, 2, 0x2001); // Attempt to blank output.
                        context.io.write_port(0x3c9, 1, 63);
                        context.io.read_port(status_port, 1);
                    }
                    let mut after_bios = [0; 0x42];
                    context.memory.read(0x449, &mut after_bios).unwrap();
                    assert_eq!(after_bios, bios_state);
                }
                let after = dos.vga().borrow().snapshot();
                let after_registers = dos.vga().borrow().registers().clone();
                assert_eq!(after_registers, before_registers);
                for plane in 0..4 {
                    assert!(std::ptr::eq(after.plane(plane), before.plane(plane)));
                }
            }
        }
    });
}

#[test]
fn bios_and_direct_addressing_controls_share_the_same_device_state() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let io = dos.cpu().context().io.clone();
        let miscellaneous = io.read_port(0x3cc, 1);
        // BIOS enable recovers from direct disable, and direct enable from BIOS disable.
        io.write_port(0x46e8, 1, 0);
        assert_eq!(
            video(&mut dos, 0x1200, 0x32, 0, 0).registers.eax as u16,
            0x1212
        );
        assert_eq!(io.read_port(0x3cc, 1), miscellaneous);
        video(&mut dos, 0x1201, 0x32, 0, 0);
        io.write_port(0x46e8, 1, 0x0e);
        assert_eq!(io.read_port(0x3cc, 1), miscellaneous);

        // BL=32h does not wake the separate option-byte sleep control.
        io.write_port(0x46e8, 1, 0x16);
        io.write_port(0x102, 1, 0);
        io.write_port(0x46e8, 1, 0x0e);
        video(&mut dos, 0x1200, 0x32, 0, 0);
        assert_eq!(io.read_port(0x3cc, 1), 0xff);
        io.write_port(0x46e8, 1, 0x16);
        io.write_port(0x102, 1, 1);
        video(&mut dos, 0x1200, 0x32, 0, 0); // Also exits setup through the same port.
        assert_eq!(io.read_port(0x3cc, 1), miscellaneous);

        // Enabling CPU access preserves both RAM disable and output blanking.
        video(&mut dos, 0x1201, 0x36, 0, 0);
        io.write_port(0x3c2, 1, miscellaneous & !2);
        let before_registers = dos.vga().borrow().registers().clone();
        video(&mut dos, 0x1201, 0x32, 0, 0);
        video(&mut dos, 0x1200, 0x32, 0, 0);
        assert_eq!(dos.vga().borrow().registers().clone(), before_registers);
        assert_eq!(io.read_port(0x3cc, 1), miscellaneous & !2);
        assert_eq!(
            dos.cpu().context().memory.read_u16(0xb8000).unwrap(),
            0xffff
        );
    });
}
