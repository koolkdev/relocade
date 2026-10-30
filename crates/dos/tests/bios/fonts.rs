//! Font services reach ordinary VGA memory and expose real ROM-backed data.
#[path = "fonts/layout.rs"]
mod layout;

use super::video::{video, PROGRAM};
use crate::common;
use relocade_x86::{Segment, SegmentRegister};

const A8: &[u8] = &[0x7c, 0xc6, 0xc6, 0xfe, 0xc6, 0xc6, 0xc6, 0];
const A14: &[u8] = &[
    0, 0, 0x38, 0x6c, 0xc6, 0xc6, 0xfe, 0xc6, 0xc6, 0xc6, 0xc6, 0, 0, 0,
];
const A16: &[u8] = &[
    0, 0, 0x38, 0x6c, 0xc6, 0xc6, 0xc6, 0xfe, 0xc6, 0xc6, 0xc6, 0xc6, 0, 0, 0, 0,
];

#[test]
fn guest_font_loading_wraps_the_source_and_preserves_other_memory_and_registers() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let glyphs = [0x80, 0x40, 0x20, 0x10, 8, 4, 3, 5, 9, 0x11, 0x21, 0x41];
        {
            let mut context = dos.cpu().context();
            context.io.write_port(0x3c4, 2, 0x0402);
            context.io.write_port(0x3c4, 2, 0x0604);
            context.io.write_port(0x3ce, 2, 0x0005);
            context.io.write_port(0x3ce, 2, 0x0506);
            context.memory.write(0xa6000, &[0xa5; 8192]).unwrap(); // Bank 5.

            // Font loading must temporarily override these CPU-access settings.
            context.io.write_port(0x3c4, 2, 0x0102);
            context.io.write_port(0x3c4, 2, 0x0e04);
            for (index, value) in [(1, 15), (3, 31), (4, 3), (5, 0x4b), (6, 0x0e), (8, 0)] {
                context.io.write_port(0x3ce, 2, index | (value << 8));
            }
            context.io.write_port(0x3c2, 1, 0x41); // VRAM access disabled.
            context
                .cpu
                .set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0x5000));
            context.cpu.set_bp(0xfffc);
            context.memory.write(0x5fffc, &glyphs[..4]).unwrap();
            context.memory.write(0x50000, &glyphs[4..]).unwrap();
            context.memory.write(0x60000, &[0xff; 8]).unwrap(); // Would expose a missing ES wrap.
        }
        let before = dos.vga().borrow().snapshot();
        let result = video(&mut dos, 0x1100, 0x0605, 2, 65);
        assert_eq!(result.registers.ebp as u16, 0xfffc);
        assert_eq!(result.segments.es.selector, 0x5000);
        let after = dos.vga().borrow().snapshot();
        assert_eq!(after.registers, before.registers);
        for plane in [0, 1, 3] {
            assert_eq!(after.plane(plane), before.plane(plane));
        }
        assert_eq!(&after.plane(2)[..0x6820], &before.plane(2)[..0x6820]);
        assert_eq!(&after.plane(2)[0x6820..0x6826], &glyphs[..6]);
        assert_eq!(&after.plane(2)[0x6826..0x6840], &[0xa5; 26]);
        assert_eq!(&after.plane(2)[0x6840..0x6846], &glyphs[6..]);
        assert_eq!(&after.plane(2)[0x6846..], &before.plane(2)[0x6846..]);
        // A retained snapshot owns the old plane-two bytes.
        assert_eq!(&before.plane(2)[0x6820..0x6840], &[0xa5; 32]);
        video(&mut dos, 0x1100, 0x0605, 0, 256);
        let unchanged = dos.vga().borrow().snapshot();
        assert_eq!(unchanged.registers, after.registers);
        assert_eq!(unchanged.plane(2), after.plane(2));
        let context = dos.cpu().context();
        assert_eq!(context.memory.read_u16(0x485).unwrap(), 8);
        assert_eq!(context.memory.read_u8(0x484).unwrap(), 24);
        assert_eq!(context.memory.read_u16(0x44c).unwrap(), 0x1000);
    });
}

#[test]
fn built_in_font_uploads_leave_geometry_and_bank_selection_to_the_caller() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (function, bank, offset, glyph) in [
            (0x1101, 4, 0x2000, A14),
            (0x1102, 1, 0x4000, A8),
            (0x1104, 7, 0xe000, A16),
        ] {
            video(&mut dos, function, bank, 99, 99);
            let snapshot = dos.vga().borrow().snapshot();
            assert_eq!(
                &snapshot.plane(2)[offset + 65 * 32..offset + 65 * 32 + glyph.len()],
                glyph
            );
            assert_eq!(
                &snapshot.plane(2)[offset + 219 * 32..offset + 219 * 32 + glyph.len()],
                vec![255; glyph.len()]
            );
            assert_eq!(snapshot.registers.sequencer.values[3], 0);
            assert_eq!(snapshot.registers.crtc.values[9], 0xc7);
        }
        video(&mut dos, 0x1103, 0xff3b, 0, 0); // Bank A=6, bank B=7.
        let snapshot = dos.vga().borrow().snapshot();
        assert_eq!(snapshot.registers.sequencer.values[3], 0x3b);
        let context = dos.cpu().context();
        assert_eq!(context.memory.read_u16(0x485).unwrap(), 8);
        assert_eq!(context.memory.read_u16(0xb8000).unwrap(), 0x0720);
    });
}

#[test]
fn font_information_returns_rom_data_current_height_and_guest_replaced_vectors() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let mut pointers = [0; 8];
        for selector in 0..8 {
            let result = video(&mut dos, 0x1130, selector << 8, 0xffff, 0xa500);
            assert_eq!(result.registers.ecx as u16, 8);
            assert_eq!(result.registers.edx as u16, 0xa518);
            let pointer = result.segments.es.base + u32::from(result.registers.ebp as u16);
            assert!((0xf0000..0xfff00).contains(&pointer));
            pointers[usize::from(selector)] = pointer;
        }
        assert_eq!(pointers[0], pointers[4]);
        assert_eq!(pointers[1], pointers[3]);
        assert_eq!(pointers[4], pointers[3] + 128 * 8);
        {
            let mut context = dos.cpu().context();
            for (selector, glyph) in [(2, A14), (3, A8), (6, A16)] {
                let mut actual = vec![0; glyph.len()];
                context
                    .memory
                    .read(pointers[selector] + 65 * glyph.len() as u32, &mut actual)
                    .unwrap();
                assert_eq!(actual, glyph);
            }
            for selector in [5, 7] {
                assert_eq!(context.memory.read_u8(pointers[selector]).unwrap(), 0);
            }
            context.memory.write_u32(0x1f * 4, 0x4321fff0).unwrap();
            context.memory.write_u32(0x43 * 4, 0x12345678).unwrap();
        }
        video(&mut dos, 0x1101, 1, 0, 0); // An offline bank load must not replace IVT43.
        for (selector, segment, offset) in [(0, 0x4321, 0xfff0), (1, 0x1234, 0x5678)] {
            let result = video(&mut dos, 0x1130, selector << 8, 0, 0);
            assert_eq!(result.segments.es.selector, segment);
            assert_eq!(result.registers.ebp as u16, offset);
        }
        video(&mut dos, 0x0012, 0, 0, 0);
        let result = video(&mut dos, 0x1130, 0x0200, 0, 0xa500); // Query 14-high ROM in 16-high graphics.
        assert_eq!(result.registers.ecx as u16, 16);
        assert_eq!(result.registers.edx as u16, 0xa51d);
        assert_eq!(
            result.segments.es.base + u32::from(result.registers.ebp as u16),
            pointers[2]
        );
        assert_eq!(
            video(&mut dos, 0x1104, 0, 0, 0).registers.eax as u16,
            0x8604
        );
    });
}

#[test]
fn invalid_font_requests_leave_memory_and_display_state_untouched() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let before = dos.vga().borrow().snapshot();
        for (ax, bx, count, first) in [
            (0x1100, 0, 1, 0),        // Zero height.
            (0x1110, 0x2100, 1, 0),   // Height beyond a hardware slot.
            (0x1102, 8, 0, 0),        // Bank beyond plane two.
            (0x1100, 0x0800, 2, 255), // Glyph range crosses the bank.
            (0x1100, 0x0800, 0, 257),
            (0x1130, 0x0800, 0, 0), // Unknown ROM selector.
            (0x11ff, 0, 0, 0),
            (0x1203, 0x30, 0, 0), // Unknown scanline preference.
        ] {
            assert_eq!(
                video(&mut dos, ax, bx, count, first).registers.eax as u16,
                0x8600 | (ax & 255)
            );
        }
        let after = dos.vga().borrow().snapshot();
        assert_eq!(after.registers, before.registers);
        for plane in 0..4 {
            assert_eq!(after.plane(plane), before.plane(plane));
        }
    });
}
