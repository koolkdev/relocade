//! Monochrome BIOS modes expose the existing text planes through B0000 and mono ports.
use super::video::{video, PROGRAM};
use crate::common;

#[test]
fn monochrome_profiles_route_ports_and_preserve_the_shared_text_planes() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (profile, height, lines, miscellaneous) in
            [(0, 14, 350, 0xa6), (1, 14, 350, 0xa6), (2, 16, 400, 0x66)]
        {
            video(&mut dos, 0x1200 | profile, 0x30, 0, 0);
            video(&mut dos, 3, 0, 0, 0);
            {
                let mut context = dos.cpu().context();
                context.memory.write_u16(0xb8000, 0x0741).unwrap();
                context.memory.write_u16(0xbfffe, 0x0f5a).unwrap();
                context.io.read_port(0x3da, 1);
                context.io.write_port(0x3c0, 1, 6); // Leave the next AC write in data phase.
            }
            let color = dos.vga().borrow().snapshot();
            assert_eq!(video(&mut dos, 0x0087, 0, 0, 0).registers.eax as u16, 0x30);
            assert_eq!(
                video(&mut dos, 0x0f00, 0, 0, 0).registers.eax as u16,
                0x5087
            );
            let mono = dos.vga().borrow().snapshot();
            assert_eq!(mono.geometry().size, [720, lines]);
            assert_eq!(mono.registers.miscellaneous, miscellaneous);
            assert_eq!(mono.registers.sequencer.values[1], 0);
            assert_eq!(mono.registers.graphics.values[6], 0x0a);
            assert_eq!(mono.registers.crtc.values[0x14], height as u8 - 1);
            assert_eq!(
                &mono.registers.attribute.values[..16],
                &[0, 8, 8, 8, 8, 8, 8, 8, 0x10, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18]
            );
            assert_eq!(mono.registers.attribute.values[0x10], 0x0e);
            assert_eq!(mono.registers.attribute.values[0x13], 8);
            for (index, level) in [
                (0, 0),
                (7, 0),
                (8, 42),
                (0x10, 42),
                (0x18, 63),
                (0x3f, 63),
                (0x40, 0),
            ] {
                assert_eq!(mono.registers.dac.colors[index], [level; 3]);
            }
            for plane in 0..2 {
                assert_eq!(mono.plane(plane), color.plane(plane));
            }
            {
                let mut context = dos.cpu().context();
                assert_eq!(context.memory.read_u16(0x463).unwrap(), 0x3b4);
                assert_eq!(context.memory.read_u16(0x465).unwrap(), 0x3029);
                assert_eq!(context.memory.read_u16(0x44c).unwrap(), 0x1000);
                assert_eq!(context.memory.read_u8(0x484).unwrap(), 24);
                assert_eq!(context.memory.read_u16(0x485).unwrap(), height);
                assert_eq!(context.memory.read_u16(0xb0000).unwrap(), 0x0741);
                assert_eq!(context.memory.read_u16(0xb7ffe).unwrap(), 0x0f5a);
                assert_eq!(context.memory.read_u16(0xb8000).unwrap(), 0xffff);
                context.memory.write_u16(0xb8000, 0x1234).unwrap();
                assert_eq!(context.memory.read_u16(0xb0000).unwrap(), 0x0741);
                assert_eq!(context.io.read_port(0x3d5, 1), 255);
                context.io.write_port(0x3b4, 1, 4);
                assert_eq!(context.io.read_port(0x3b5, 1), 0x55);
            }
            let before = dos.vga().borrow().snapshot();
            for bx in [0x001f, 0x0100, 0xffaa] {
                assert_eq!(
                    video(&mut dos, 0x0b00, bx, 0, 0).registers.eax as u16,
                    0x0b00
                );
            }
            assert_eq!(dos.vga().borrow().snapshot().registers, before.registers);
            assert_eq!(dos.cpu().context().memory.read_u8(0x466).unwrap(), 0x30);
            // VGA palette services still work through the monochrome status port.
            video(&mut dos, 0x1000, 0x1801, 0, 0);
            assert_eq!(
                dos.vga().borrow().snapshot().registers.attribute.values[1],
                0x18
            );
            video(&mut dos, 7, 0, 0, 0);
            {
                let mut context = dos.cpu().context();
                assert_eq!(context.memory.read_u16(0xb0000).unwrap(), 0x0720);
                assert_eq!(context.memory.read_u16(0xb7ffe).unwrap(), 0x0720);
                context.memory.write_u16(0xb0000, 0x1e51).unwrap();
            }
            video(&mut dos, 0x0083, 0, 0, 0);
            let context = dos.cpu().context();
            assert_eq!(context.memory.read_u16(0xb8000).unwrap(), 0x1e51);
            assert_eq!(context.memory.read_u16(0x463).unwrap(), 0x3d4);
            // Mono's fallback to 350 lines does not overwrite the color preference.
            assert_eq!(
                context.memory.read_u16(0x485).unwrap(),
                if profile == 0 { 8 } else { height }
            );
        }
    });
}

#[test]
fn monochrome_pages_scroll_and_font_changes_keep_the_mono_aperture_and_underline() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 7, 0, 0, 0);
        video(&mut dos, 0x0200, 0x0700, 0, 0x184f);
        video(&mut dos, 0x0958, 0x070f, 1, 0);
        assert_eq!(
            video(&mut dos, 0x0800, 0x0700, 0, 0).registers.eax as u16,
            0x0f58
        );
        video(&mut dos, 0x0a59, 0x0700, 1, 0);
        assert_eq!(
            dos.cpu().context().memory.read_u16(0xb7f9e).unwrap(),
            0x0f59
        );
        video(&mut dos, 0x0507, 0, 0, 0);
        let capture = common::next_capture(&dos).await;
        assert_eq!(capture.display.registers.crtc.display_start, 0x3800);
        assert_eq!(
            &capture.display.registers.crtc.values[14..16],
            &[0x3f, 0xcf]
        );
        video(&mut dos, 0x0e5a, 0, 0, 0);
        assert_eq!(
            dos.cpu().context().memory.read_u16(0xb7efe).unwrap(),
            0x0f5a
        );
        assert_eq!(
            dos.cpu().context().memory.read_u16(0xb7f00).unwrap(),
            0x0720
        );
        assert_eq!(
            dos.cpu().context().memory.read_u16(0xb0000).unwrap(),
            0x0720
        );
        assert_eq!(
            video(&mut dos, 0x0508, 0, 0, 0).registers.eax as u16,
            0x8608
        );
        video(&mut dos, 0x0500, 0, 0, 0);
        for (function, height, rows, page_bytes) in
            [(0x1112, 8, 43, 0x2000), (0x1111, 14, 25, 0x1000)]
        {
            let before = dos.vga().borrow().snapshot();
            video(&mut dos, function, 0, 0, 0);
            let after = dos.vga().borrow().snapshot();
            assert_eq!(after.registers.crtc.values[0x14], height - 1);
            assert_eq!(after.registers.crtc.values[9] & 31, height - 1);
            assert_eq!(
                after.registers.graphics.values,
                before.registers.graphics.values
            );
            assert_eq!(
                after.registers.sequencer.values,
                before.registers.sequencer.values
            );
            assert_eq!(after.registers.miscellaneous, 0xa6);
            for plane in 0..2 {
                assert_eq!(after.plane(plane), before.plane(plane));
            }
            let context = dos.cpu().context();
            assert_eq!(context.memory.read_u8(0x484).unwrap(), rows - 1);
            assert_eq!(context.memory.read_u16(0x44c).unwrap(), page_bytes);
            assert_eq!(context.memory.read_u16(0xb7efe).unwrap(), 0x0f5a);
            assert_eq!(context.memory.read_u16(0xb8000).unwrap(), 0xffff);
        }
    });
}
