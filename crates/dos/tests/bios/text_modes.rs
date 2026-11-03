//! BIOS color text modes program the hardware and the firmware geometry together.
use super::video::{video, PROGRAM};
use crate::common;

#[test]
fn startup_uses_the_400_line_color_profile_without_overriding_later_guest_choices() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let initial = common::next_capture(&dos).await;
        assert_eq!(initial.display.geometry().size, [738, 414]);
        assert_eq!(initial.display.geometry().pixel_aspect_ratio(), (20, 27));
        assert_eq!(
            &initial.display.plane(2)[65 * 32..65 * 32 + 16],
            &[0, 0, 0x38, 0x6c, 0xc6, 0xc6, 0xc6, 0xfe, 0xc6, 0xc6, 0xc6, 0xc6, 0, 0, 0, 0,]
        );
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        assert_eq!(
            video(&mut dos, 0x0f00, 0, 0, 0).registers.eax as u16,
            0x5003
        );
        assert_eq!(
            video(&mut dos, 0x0300, 0, 0, 0).registers.ecx as u16,
            0x0d0e
        );
        assert_eq!(dos.cpu().context().memory.read_u16(0x485).unwrap(), 16);
        assert_eq!(dos.cpu().context().memory.read_u8(0x484).unwrap(), 24);

        // A guest's later preference survives a graphics mode and the return to text.
        video(&mut dos, 0x1200, 0x30, 0, 0);
        video(&mut dos, 0x13, 0, 0, 0);
        video(&mut dos, 3, 0, 0, 0);
        let legacy = common::next_capture(&dos).await;
        assert_eq!(legacy.display.geometry().size, [656, 414]);
        assert_eq!(dos.cpu().context().memory.read_u16(0x485).unwrap(), 8);
        assert_eq!(initial.display.geometry().size, [738, 414]);
    });
}

#[test]
fn color_text_modes_apply_scanline_profiles_and_report_their_page_geometry() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (mode, columns, page_bytes, shadow) in [
            (0, 40, 0x0800, 0x2c),
            (1, 40, 0x0800, 0x28),
            (2, 80, 0x1000, 0x2d),
            (3, 80, 0x1000, 0x29),
        ] {
            for (profile, font_height, lines) in [(0, 8, 200), (1, 14, 350), (2, 16, 400)] {
                video(&mut dos, 0x1200 | profile, 0x30, 0, 0);
                assert_eq!(video(&mut dos, mode, 0, 0, 0).registers.eax as u16, 0x30);
                assert_eq!(
                    video(&mut dos, 0x0f00, 0, 0, 0).registers.eax as u16,
                    (columns << 8) | mode
                );
                let display = dos.vga().borrow().snapshot();
                let cell_width = if profile == 2 { 9 } else { 8 };
                assert_eq!(
                    display.geometry().active_size,
                    [
                        u32::from(columns) * cell_width,
                        if lines == 200 { 400 } else { lines }
                    ]
                );
                let clocking = u8::from(columns == 40) * 8 + u8::from(profile != 2);
                assert_eq!(display.registers.sequencer.values[1], clocking);
                if profile == 2 {
                    assert_eq!(display.registers.miscellaneous, 0x67);
                    assert_eq!(display.registers.attribute.values[0x13], 8);
                    let crtc = &display.registers.crtc.values;
                    let retrace = if columns == 40 {
                        [0x2b, 0xa0]
                    } else {
                        [0x55, 0x81]
                    };
                    assert_eq!([crtc[4], crtc[5]], retrace);
                    assert_eq!([crtc[0x0a], crtc[0x0b]], [13, 14]);
                    assert_eq!(crtc[0x11], 0x8e);
                }
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
