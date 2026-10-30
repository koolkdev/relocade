use super::video::{video, PROGRAM};
use crate::common;

#[test]
fn mode_13h_restores_memory_routing_geometry_and_default_palette() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        assert_eq!(video(&mut dos, 0x0013, 0, 0, 0).registers.eax as u16, 0x20);
        {
            let mut context = dos.cpu().context();
            context.memory.write(0xa0000, &[0xa5; 65536]).unwrap();
            context.memory.write(0x450, &[0xff; 19]).unwrap();
            // Leave disabled planes, nonzero write modes, monochrome ports,
            // protected timing, panning and unfinished attribute/DAC writes.
            for (port, word) in [
                (0x3c4, 0x0002),
                (0x3c4, 0x2801),
                (0x3c4, 0x0004),
                (0x3ce, 0x0f01),
                (0x3ce, 0x1f03),
                (0x3ce, 0x4b05),
                (0x3ce, 0x0e06),
                (0x3ce, 0x0008),
                (0x3d4, 0x2011),
                (0x3d4, 0x1f08),
                (0x3d4, 0x8011),
            ] {
                context.io.write_port(port, 2, word);
            }
            context.io.read_port(0x3da, 1);
            context.io.write_port(0x3c0, 1, 0x14);
            context.io.write_port(0x3c8, 1, 6);
            context.io.write_port(0x3c9, 1, 0);
            context.io.write_port(0x3c6, 1, 0);
            context.io.write_port(0x3c2, 1, 0x46);
        }
        video(&mut dos, 0x0013, 0, 0, 0);
        let display = dos.vga().borrow().snapshot();
        assert_eq!(display.geometry().size, [320, 200]);
        assert_eq!(display.geometry().scanline_step, 2);
        assert_eq!(display.geometry().pixel_aspect_ratio(), (5, 6));
        let mode = video(&mut dos, 0x0f00, 0xffa5, 0, 0);
        assert_eq!(mode.registers.eax as u16, 0x2813);
        assert_eq!(mode.registers.ebx as u16, 0x00a5);
        let context = dos.cpu().context();
        let mut pixels = [0xff; 65536];
        context.memory.read(0xa0000, &mut pixels).unwrap();
        assert!(pixels.iter().all(|&pixel| pixel == 0));
        assert_eq!(context.memory.read_u8(0xb8000).unwrap(), 0xff);
        assert_eq!(context.memory.read_u16(0x44c).unwrap(), 64000);
        assert_eq!(context.memory.read_u16(0x44e).unwrap(), 0);
        assert_eq!(context.memory.read_u16(0x463).unwrap(), 0x3d4);
        assert_eq!(context.memory.read_u8(0x484).unwrap(), 24);
        assert_eq!(context.memory.read_u16(0x485).unwrap(), 8);
        assert_eq!(context.io.read_port(0x3cc, 1), 0x63);
        for (port, index, expected) in [
            (0x3c4, 0, 3),
            (0x3c4, 1, 1),
            (0x3c4, 2, 15),
            (0x3c4, 4, 14),
            (0x3ce, 1, 0),
            (0x3ce, 3, 0),
            (0x3ce, 5, 0x40),
            (0x3ce, 6, 5),
            (0x3ce, 8, 255),
            (0x3d4, 0, 0x5f),
            (0x3d4, 8, 0),
            (0x3d4, 0x11, 0x8e),
        ] {
            context.io.write_port(port, 1, index);
            assert_eq!(context.io.read_port(port + 1, 1), expected);
        }
        assert_eq!(context.io.read_port(0x3c0, 1) & 0x20, 0x20);
        assert_eq!(context.io.read_port(0x3c6, 1), 255);
        // PC brown/bright colors, grayscale levels, hue-ring transitions and
        // final reserved entries have distinct firmware defaults.
        for (index, rgb) in [
            (6, [42, 21, 0]),
            (9, [21, 21, 63]),
            (15, [63; 3]),
            (17, [5; 3]),
            (30, [56; 3]),
            (32, [0, 0, 63]),
            (33, [16, 0, 63]),
            (40, [63, 0, 0]),
            (55, [0, 16, 63]),
            (56, [31, 31, 63]),
            (80, [45, 45, 63]),
            (104, [0, 0, 28]),
            (128, [14, 14, 28]),
            (152, [20, 20, 28]),
            (176, [0, 0, 16]),
            (200, [8, 8, 16]),
            (226, [13, 11, 16]),
            (247, [11, 12, 16]),
            (248, [0; 3]),
            (255, [0; 3]),
        ] {
            context.io.write_port(0x3c7, 1, index);
            for component in rgb {
                assert_eq!(context.io.read_port(0x3c9, 1), component, "DAC {index}");
            }
        }
    });
}

#[test]
fn preserve_memory_flag_queries_and_text_return_follow_bios_bookkeeping() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x0013, 0, 0, 0);
        let pattern: Vec<_> = (0..65536).map(|offset| (offset * 7 + 11) as u8).collect();
        dos.cpu().context().memory.write(0xa0000, &pattern).unwrap();
        video(&mut dos, 0x0093, 0, 0, 0);
        let mode = video(&mut dos, 0x0f00, 0, 0, 0);
        assert_eq!(mode.registers.eax as u16, 0x2893);
        let mut actual = vec![0; 65536];
        dos.cpu()
            .context()
            .memory
            .read(0xa0000, &mut actual)
            .unwrap();
        assert_eq!(actual, pattern);
        assert_eq!(dos.cpu().context().memory.read_u8(0x449).unwrap(), 0x13);

        // Unsupported modes and text services cannot corrupt graphics state.
        let retained = dos.vga().borrow().snapshot();
        for ax in [0x0012, 0x0507, 0x0941, 0x0e41] {
            assert_eq!(video(&mut dos, ax, 0, 1, 0).registers.eax as u16 >> 8, 0x86);
        }
        {
            let mut context = dos.cpu().context();
            relocade_bios::write_text(&mut context.memory, context.io, b"text");
        }
        let display = dos.vga().borrow().snapshot();
        for plane in 0..4 {
            assert_eq!(display.plane(plane), retained.plane(plane));
        }
        assert_eq!(
            display.registers.crtc.values,
            retained.registers.crtc.values
        );
        assert_eq!(display.registers.dac.colors, retained.registers.dac.colors);

        video(&mut dos, 0x0003, 0, 0, 0);
        assert_eq!(
            video(&mut dos, 0x0f00, 0, 0, 0).registers.eax as u16,
            0x5003
        );
        video(&mut dos, 0x0958, 0x002e, 1, 0);
        video(&mut dos, 0x0083, 0, 0, 0);
        assert_eq!(
            video(&mut dos, 0x0f00, 0, 0, 0).registers.eax as u16,
            0x5083
        );
        assert_eq!(
            dos.cpu().context().memory.read_u16(0xb8000).unwrap(),
            0x2e58
        );
        let text = dos.vga().borrow().snapshot();
        assert_eq!(
            &text.plane(2)[65 * 32..65 * 32 + 8],
            &[0x7c, 0xc6, 0xc6, 0xfe, 0xc6, 0xc6, 0xc6, 0]
        );
        assert_eq!(text.geometry().size, [640, 200]);
        // Loading the text font cannot modify a retained graphics capture.
        assert_ne!(retained.plane(2), text.plane(2));
    });
}
