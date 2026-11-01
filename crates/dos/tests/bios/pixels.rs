use super::video::{video, PROGRAM};
use crate::common;

#[test]
fn indexed_pixel_access_restores_hostile_guest_write_settings() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x0013, 0, 0, 0);
        for (port, word) in [
            (0x3c4, 0x0002),
            (0x3ce, 0x0f01),
            (0x3ce, 0x1b03),
            (0x3ce, 0x4b05),
            (0x3ce, 0x5a08),
        ] {
            dos.cpu().context().io.write_port(port, 2, word);
        }
        let before = dos.vga().borrow().snapshot();
        video(&mut dos, 0x0c85, 0, 0, 0);
        assert_eq!(video(&mut dos, 0x0d00, 0, 0, 0).registers.eax as u8, 0x85);
        let after = dos.vga().borrow().snapshot();
        assert_eq!(after.plane(0)[0], 0x85);
        assert_eq!(after.registers, before.registers);
    });
}

#[test]
fn planar_pixel_calls_preserve_neighbors_and_restore_access_registers() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for mode in [0x0d, 0x0f, 0x11, 0x12] {
            video(&mut dos, mode, 0, 0, 0);
            {
                let context = dos.cpu().context();
                // BIOS must temporarily override write masks/mode and read mode,
                // without changing the caller's settings on return.
                for (port, value) in [
                    (0x3c4, 0x0502),
                    (0x3ce, 0x1b03),
                    (0x3ce, 0x0304),
                    (0x3ce, 0x0b05),
                    (0x3ce, 0x5a08),
                    (0x3ce, 0x0f01),
                ] {
                    context.io.write_port(port, 2, value);
                }
            }
            let before = dos.vga().borrow().snapshot();
            // Neighboring pixels include a byte boundary, four distinct color
            // bits, replacement with zero, and reversible XOR.
            for (x, color) in [(6, 15), (7, 5), (8, 10), (9, 3)] {
                video(&mut dos, 0x0c00 | color, 0, x, 2);
            }
            for (x, color) in [(6, 15), (7, 5), (8, 10), (9, 3)] {
                let cpu = video(&mut dos, 0x0da5, 0x005a, x, 2);
                assert_eq!(cpu.registers.eax as u16, 0x0d00 | color);
                assert_eq!(cpu.registers.ebx as u16, 0x005a);
                assert_eq!(cpu.registers.ecx as u16, x);
                assert_eq!(cpu.registers.edx as u16, 2);
            }
            for expected in [3, 5] {
                video(&mut dos, 0x0c86, 0, 7, 2);
                assert_eq!(
                    video(&mut dos, 0x0d00, 0, 7, 2).registers.eax as u8,
                    expected
                );
            }
            video(&mut dos, 0x0c00, 0, 8, 2);
            assert_eq!(video(&mut dos, 0x0d00, 0, 8, 2).registers.eax as u8, 0);
            let after = dos.vga().borrow().snapshot();
            assert_eq!(
                before.registers.graphics.values,
                after.registers.graphics.values
            );
            assert_eq!(
                before.registers.sequencer.values,
                after.registers.sequencer.values
            );
            let offset = if mode == 0x0d { 80 } else { 160 };
            for (plane, byte) in [3, 2, 3, 2].into_iter().enumerate() {
                assert_eq!(after.plane(plane)[offset], byte); // Pixels 6=15, 7=5.
                assert_eq!(
                    after.plane(plane)[offset + 1],
                    if plane < 2 { 0x40 } else { 0 }
                );
                assert_eq!(before.plane(plane)[offset], 0); // Retained capture.
            }
        }
    });
}

#[test]
fn pixel_pages_and_256_color_high_bits_follow_the_bios_abi() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (mode, pages, page_bytes, x, y, last_byte) in [
            (0x0d, 8, 0x2000, 319, 199, 7999),
            (0x0e, 4, 0x4000, 639, 199, 15999),
            (0x0f, 2, 0x8000, 639, 349, 27999),
            (0x10, 2, 0x8000, 639, 349, 27999),
        ] {
            video(&mut dos, mode, 0, 0, 0);
            for page in 0..pages {
                video(&mut dos, 0x0c00 | (page + 1), page << 8, x, y);
            }
            // Changing the display start does not change the page requested by BH.
            let crtc_port = dos.cpu().context().memory.read_u16(0x463).unwrap();
            dos.cpu().context().io.write_port(crtc_port, 2, 0x600c);
            let snapshot = dos.vga().borrow().snapshot();
            for page in 0..pages {
                assert_eq!(
                    video(&mut dos, 0x0d00, page << 8, x, y).registers.eax as u8,
                    page as u8 + 1
                );
                for plane in 0..4 {
                    assert_eq!(
                        snapshot.plane(plane)[usize::from(page) * page_bytes + last_byte],
                        ((page + 1) >> plane) as u8 & 1
                    );
                }
            }
        }
        for (mode, x, y, color, offset) in
            [(0x12, 639, 479, 15, 38399), (0x13, 319, 199, 0x80, 63999)]
        {
            video(&mut dos, mode, 0, 0, 0);
            video(&mut dos, 0x0c00 | color, 0xff00, x, y);
            assert_eq!(
                video(&mut dos, 0x0d00, 0xa500, x, y).registers.eax as u8,
                color as u8
            );
            if mode == 0x13 {
                assert_eq!(
                    dos.cpu()
                        .context()
                        .memory
                        .read_u8(0xa0000 + offset)
                        .unwrap(),
                    0x80
                );
                // The same high-bit write twice still stores the color.
                video(&mut dos, 0x0c80, 0, x, y);
                assert_eq!(video(&mut dos, 0x0d00, 0, x, y).registers.eax as u8, 0x80);
            } else {
                let display = dos.vga().borrow().snapshot();
                for plane in 0..4 {
                    assert_eq!(display.plane(plane)[offset as usize], 1);
                }
            }
        }
    });
}

#[test]
fn unsupported_pixel_calls_leave_video_memory_untouched() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (mode, page, x, y) in [
            (3, 0, 0, 0),
            (0x0d, 8, 0, 0),
            (0x0d, 0, 320, 0),
            (0x0e, 4, 0, 0),
            (0x0e, 0, 0, 200),
            (0x0f, 2, 0, 0),
            (0x0f, 0, 0, 350),
            (0x10, 2, 0, 0),
            (0x10, 0, 0, 350),
            (0x11, 0, 0, 480),
            (0x12, 0, 0, 480),
            (0x13, 0, 320, 0),
            (0x13, 0, 0, 200),
        ] {
            video(&mut dos, mode, 0, 0, 0);
            let before = dos.vga().borrow().snapshot();
            for function in [0x0c5a, 0x0d5a] {
                assert_eq!(
                    video(&mut dos, function, page << 8, x, y).registers.eax as u16,
                    0x865a
                );
            }
            let after = dos.vga().borrow().snapshot();
            for plane in 0..4 {
                assert_eq!(before.plane(plane), after.plane(plane));
            }
            assert_eq!(before.registers, after.registers);
        }
        // Unlike the normal helper, enter with carry clear to observe failure
        // status being written into the interrupt return frame.
        {
            let context = dos.cpu().context();
            context.cpu.set_eip(0x100);
            context.cpu.set_ax(0x0c5a);
            context.cpu.set_cx(320); // Outside mode 13h.
            context.cpu.set_dx(0);
            context.cpu.set_eflags(0x642);
        }
        assert_eq!(
            dos.cpu().run(15).await,
            relocade_x86::ExecutionExit::BudgetExhausted
        );
        let cpu = dos.cpu().context().cpu.snapshot();
        assert_eq!(cpu.registers.eax as u16, 0x865a);
        assert_eq!(cpu.flags.eflags(), 0x643);
    });
}
