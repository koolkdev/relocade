//! BIOS consumers follow the firmware geometry independently of VGA scanout.
use super::video::{video, PROGRAM};
use crate::common;

#[test]
fn text_pages_cursor_and_character_access_follow_bios_geometry() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        {
            let mut context = dos.cpu().context();
            context.memory.write_u16(0x44a, 80).unwrap();
            context.memory.write_u16(0x44c, 0x2000).unwrap();
            context.memory.write_u8(0x484, 49).unwrap();
        }
        video(&mut dos, 0x0200, 0x0300, 0, 0x314f); // Page 3, last cell of 80x50.
        video(&mut dos, 0x0958, 0x032e, 1, 0);
        assert_eq!(
            video(&mut dos, 0x0800, 0x0300, 0, 0).registers.eax as u16,
            0x2e58
        );
        video(&mut dos, 0x0503, 0, 0, 0);
        let display = common::next_capture(&dos).await;
        assert_eq!(display.display.registers.crtc.display_start, 0x3000);
        let cursor = &display.display.registers.crtc.values;
        assert_eq!(u16::from_be_bytes([cursor[14], cursor[15]]), 0x3f9f);
        {
            let context = dos.cpu().context();
            assert_eq!(context.memory.read_u16(0x44e).unwrap(), 0x6000);
            assert_eq!(context.memory.read_u16(0xbff3e).unwrap(), 0x2e58);
            assert_eq!(context.memory.read_u16(0xb8000).unwrap(), 0x0720);
        }
        // Eight cursor records do not imply eight 8-KiB text pages.
        assert_eq!(
            video(&mut dos, 0x0504, 0, 0, 0).registers.eax as u16,
            0x8604
        );
        assert_eq!(
            video(&mut dos, 0x0200, 0x0400, 0, 0).registers.eax as u16,
            0x8600
        );
        assert_eq!(
            video(&mut dos, 0x0f00, 0, 0, 0).registers.ebx as u16,
            0x0300
        );
    });
}

#[test]
fn teletype_and_scrolling_use_programmed_columns_and_rows() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        {
            let mut context = dos.cpu().context();
            context.memory.write_u16(0x44a, 40).unwrap();
            context.memory.write_u8(0x484, 49).unwrap();
            context.memory.write_u16(0xb8000 + 49 * 80, 0x2a20).unwrap();
        }
        video(&mut dos, 0x0200, 0, 0, 0x3127); // Last cell of 40x50.
        video(&mut dos, 0x0920, 0x001f, 1, 0);
        video(&mut dos, 0x0e5a, 0, 0, 0);
        {
            let context = dos.cpu().context();
            assert_eq!(
                context.memory.read_u16(0xb8000 + 48 * 80 + 78).unwrap(),
                0x1f5a
            );
            assert_eq!(context.memory.read_u16(0xb8000 + 49 * 80).unwrap(), 0x2a20);
            assert_eq!(
                context.memory.read_u16(0xb8000 + 49 * 80 + 78).unwrap(),
                0x2a20
            );
            assert_eq!(context.memory.read_u16(0xb8000 + 4000).unwrap(), 0x0720);
            // Page padding.
        }
        assert_eq!(
            video(&mut dos, 0x0300, 0, 0, 0).registers.edx as u16,
            0x3100
        );
        // Window coordinates are clipped to the BIOS width and last row.
        video(&mut dos, 0x0600, 0x4e00, 0x3126, 0xffff);
        let context = dos.cpu().context();
        assert_eq!(
            context.memory.read_u16(0xb8000 + 49 * 80 + 74).unwrap(),
            0x2a20
        );
        assert_eq!(
            context.memory.read_u16(0xb8000 + 49 * 80 + 76).unwrap(),
            0x4e20
        );
        assert_eq!(context.memory.read_u16(0xb8000 + 4000).unwrap(), 0x0720);
    });
}

#[test]
fn scrolling_copies_a_wide_window_without_the_old_eighty_column_limit() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        {
            let mut context = dos.cpu().context();
            context.memory.write_u16(0x44a, 132).unwrap();
            context.memory.write_u16(0x44c, 0x4000).unwrap();
            for (row, value) in [(0, 0x1e41), (1, 0x2f42)] {
                context
                    .memory
                    .write_u16(0xb8000 + row * 264 + 262, value)
                    .unwrap();
            }
        }
        video(&mut dos, 0x0601, 0x3a00, 0, 0x0183);
        let context = dos.cpu().context();
        assert_eq!(context.memory.read_u16(0xb8106).unwrap(), 0x2f42);
        assert_eq!(context.memory.read_u16(0xb820e).unwrap(), 0x3a20);
        assert_eq!(context.memory.read_u16(0xb8210).unwrap(), 0x0720);
    });
}
