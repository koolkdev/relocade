//! BIOS color text modes program the hardware and the firmware geometry together.
use super::video::{video, PROGRAM};
use crate::common;

#[test]
fn color_text_modes_apply_scanline_profiles_and_report_their_page_geometry() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (mode, columns, page_bytes, width, shadow, clocking) in [
            (0, 40, 0x0800, 320, 0x2c, 9),
            (1, 40, 0x0800, 320, 0x28, 9),
            (2, 80, 0x1000, 640, 0x2d, 1),
            (3, 80, 0x1000, 640, 0x29, 1),
        ] {
            for (profile, font_height, lines) in [(0, 8, 200), (1, 14, 350), (2, 16, 400)] {
                video(&mut dos, 0x1200 | profile, 0x30, 0, 0);
                assert_eq!(video(&mut dos, mode, 0, 0, 0).registers.eax as u16, 0x30);
                assert_eq!(
                    video(&mut dos, 0x0f00, 0, 0, 0).registers.eax as u16,
                    (columns << 8) | mode
                );
                let display = dos.vga().borrow().snapshot();
                assert_eq!(display.geometry().size, [width, lines]);
                assert_eq!(display.registers.sequencer.values[1], clocking);
                assert_eq!(display.registers.crtc.values[1], columns as u8 - 1);
                assert_eq!(display.registers.crtc.values[0x13], columns as u8 / 2);
                assert_eq!(display.registers.graphics.values[6], 0x0e);
                assert_eq!(display.registers.attribute.values[6], 0x14);
                assert_eq!(display.registers.dac.colors[0x14], [42, 21, 0]);
                let context = dos.cpu().context();
                assert_eq!(context.memory.read_u16(0x44c).unwrap(), page_bytes);
                assert_eq!(context.memory.read_u8(0x484).unwrap(), 24);
                assert_eq!(context.memory.read_u16(0x485).unwrap(), font_height);
                assert_eq!(context.memory.read_u16(0x463).unwrap(), 0x3d4);
                assert_eq!(context.memory.read_u16(0x465).unwrap(), 0x3000 | shadow);
                assert_eq!(context.memory.read_u16(0xbfffe).unwrap(), 0x0720);
            }
        }
    });
}

#[test]
fn forty_column_pages_wrap_scroll_and_preserve_text_on_mode_changes() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 1, 0, 0, 0);
        video(&mut dos, 0x0507, 0, 0, 0);
        video(&mut dos, 0x0200, 0x0700, 0, 0x1827); // Last cell on page seven.
        video(&mut dos, 0x0920, 0x071f, 1, 0);
        dos.cpu()
            .context()
            .memory
            .write_u16(0xbbf80, 0x2a20)
            .unwrap();
        video(&mut dos, 0x0e5a, 0, 0, 0); // Wrap and scroll the active page.
        assert_eq!(
            video(&mut dos, 0x0300, 0x0700, 0, 0).registers.edx as u16,
            0x1800
        );
        assert_eq!(
            video(&mut dos, 0x0800, 0x0700, 0, 0).registers.eax as u16,
            0x2a20
        );
        let capture = common::next_capture(&dos).await;
        assert_eq!(capture.display.registers.crtc.display_start, 0x1c00);
        let crtc = &capture.display.registers.crtc.values;
        assert_eq!([crtc[14], crtc[15]], [0x1f, 0xc0]);
        {
            let context = dos.cpu().context();
            assert_eq!(context.memory.read_u16(0x44e).unwrap(), 0x3800);
            assert_eq!(context.memory.read_u16(0xbbf7e).unwrap(), 0x1f5a);
            assert_eq!(context.memory.read_u16(0xbbfce).unwrap(), 0x2a20);
            assert_eq!(context.memory.read_u16(0xbbfd0).unwrap(), 0x0720); // Page padding.
            assert_eq!(context.memory.read_u16(0xb8000).unwrap(), 0x0720); // Inactive page.
        }
        assert_eq!(
            video(&mut dos, 0x0508, 0, 0, 0).registers.eax as u16,
            0x8608
        );
        video(&mut dos, 0x0080, 0, 0, 0); // Mode zero uses the same color hardware state.
        assert_eq!(
            video(&mut dos, 0x0f00, 0, 0, 0).registers.eax as u16,
            0x2880
        );
        assert_eq!(
            dos.cpu().context().memory.read_u16(0xbbf7e).unwrap(),
            0x1f5a
        );
        video(&mut dos, 0, 0, 0, 0);
        assert_eq!(
            dos.cpu().context().memory.read_u16(0xbbf7e).unwrap(),
            0x0720
        );

        // Font-height changes use forty-column page allocation, including all eight pages.
        video(&mut dos, 0x1202, 0x30, 0, 0);
        video(&mut dos, 1, 0, 0, 0);
        video(&mut dos, 0x1112, 0, 0, 0);
        assert_eq!(dos.cpu().context().memory.read_u8(0x484).unwrap(), 49);
        assert_eq!(dos.cpu().context().memory.read_u16(0x44c).unwrap(), 0x1000);
        video(&mut dos, 0x0200, 0x0700, 0, 0x3127);
        video(&mut dos, 0x0958, 0x071e, 1, 0);
        assert_eq!(
            dos.cpu().context().memory.read_u16(0xbff9e).unwrap(),
            0x1e58
        );
    });
}
