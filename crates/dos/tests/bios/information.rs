use super::video::{video, PROGRAM};
use crate::common;
use relocade_dos::Dos;
use relocade_x86::{Segment, SegmentRegister};

pub(super) fn information(dos: &mut Dos, segment: u16, offset: u16, selector: u16) -> [u8; 64] {
    {
        let context = dos.cpu().context();
        context.cpu.set_segment(
            Segment::Es,
            SegmentRegister::real_mode(Segment::Es, segment),
        );
        context.cpu.set_di(offset);
        context.cpu.set_si(0xa5c3);
        context.cpu.set_bp(0x789a);
    }
    // Both supported and unsupported selectors must replace the incoming AL.
    let result = video(dos, 0x1ba5, selector, 0x1234, 0x5678);
    assert_eq!(
        result.registers.eax as u16,
        if selector == 0 { 0x1b1b } else { 0x1b00 }
    );
    assert_eq!(result.registers.ebx as u16, selector);
    assert_eq!(result.registers.ecx as u16, 0x1234);
    assert_eq!(result.registers.edx as u16, 0x5678);
    assert_eq!(result.registers.esi as u16, 0xa5c3);
    assert_eq!(result.registers.ebp as u16, 0x789a);
    assert_eq!(result.registers.edi as u16, offset);
    assert_eq!(result.segments.es.selector, segment);
    let context = dos.cpu().context();
    std::array::from_fn(|index| {
        let address = (u32::from(segment) << 4) + u32::from(offset.wrapping_add(index as u16));
        context.memory.read_u8(address).unwrap()
    })
}

#[test]
fn functionality_report_has_the_ibm_layout_and_rom_capabilities() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x0100, 0, 0x0207, 0);
        video(&mut dos, 0x0200, 0x0700, 0, 0x1234);
        video(&mut dos, 0x0507, 0, 0, 0);
        {
            let mut context = dos.cpu().context();
            context.memory.write(0x501ff, &[0xa5; 66]).unwrap();
            // The query must preserve the sequencer index and attribute address/PAS.
            context.io.write_port(0x3c4, 1, 4);
            context.io.read_port(0x3da, 1);
            context.io.write_port(0x3c0, 1, 0x27);
        }
        let before = dos.vga().borrow().snapshot();
        let bytes = information(&mut dos, 0x5000, 0x200, 0);
        assert_eq!(&bytes[4..0x0b], &[3, 80, 0, 0, 0x10, 0, 0x70]);
        assert_eq!(&bytes[0x0b..0x19], &[0; 14]); // Inactive cursors 0–6.
        assert_eq!(
            &bytes[0x19..0x22],
            &[0x34, 0x12, 7, 2, 7, 0xd4, 3, 0x29, 0x30]
        );
        assert_eq!(
            &bytes[0x22..0x2e],
            &[25, 8, 0, 8, 0, 16, 0, 8, 0, 0, 0, 0x21]
        );
        assert_eq!(&bytes[0x2e..0x31], &[0; 3]);
        assert_eq!(bytes[0x31], 3); // 256 KiB.
        assert_eq!(&bytes[0x32..], &[0; 14]); // No save-pointer overrides or extensions.
        let pointer = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        assert_eq!(pointer >> 16, 0xf000);
        let address = (pointer >> 16) * 16 + (pointer & 0xffff);
        let context = dos.cpu().context();
        let mut capabilities = [0; 16];
        context.memory.read(address, &mut capabilities).unwrap();
        assert_eq!(
            capabilities,
            [0xff, 0xe0, 0x0f, 0, 0, 0, 0, 7, 2, 8, 0xfe, 0x0e, 0, 0, 0, 0]
        );
        assert_eq!(context.memory.read_u8(0x501ff).unwrap(), 0xa5);
        assert_eq!(context.memory.read_u8(0x50240).unwrap(), 0xa5);
        let after = dos.vga().borrow().snapshot();
        assert_eq!(after.registers, before.registers);
        for plane in 0..4 {
            assert!(std::ptr::eq(after.plane(plane), before.plane(plane)));
        }
    });
}

#[test]
fn state_report_follows_mode_font_page_and_live_register_changes() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (mode, colors, pages, scanlines) in [
            (3, 16, 8, 0),
            (4, 4, 1, 0),
            (6, 2, 1, 0),
            (7, 0, 8, 1),
            (0x0d, 16, 8, 0),
            (0x0e, 16, 4, 0),
            (0x0f, 0, 2, 1),
            (0x10, 16, 2, 1),
            (0x11, 2, 1, 3),
            (0x12, 16, 1, 3),
            (0x13, 256, 1, 0),
        ] {
            video(&mut dos, mode, 0, 0, 0);
            let bytes = information(&mut dos, 0x5000, 0x200, 0);
            assert_eq!(bytes[4], mode as u8);
            assert_eq!(&bytes[0x25..0x27], &[8, 0]); // Color VGA, even in mono modes.
            assert_eq!(u16::from_le_bytes([bytes[0x27], bytes[0x28]]), colors);
            assert_eq!(bytes[0x29], pages);
            assert_eq!(bytes[0x2a], scanlines);
        }
        for (selection, rows) in [(1, 43), (2, 50)] {
            video(&mut dos, 0x1200 | selection, 0x30, 0, 0);
            video(&mut dos, 3, 0, 0, 0);
            video(&mut dos, 0x1112, 0, 0, 0); // 8-high font changes row and page counts.
            let bytes = information(&mut dos, 0x5000, 0x200, 0);
            assert_eq!(&bytes[0x22..0x25], &[rows, 8, 0]);
            assert_eq!(bytes[0x29], 4);
            assert_eq!(bytes[0x2a], selection as u8); // 350 lines, not 43 × 8 = 344.
        }
        video(&mut dos, 0x1103, 0x3b, 0, 0); // Normal bank B=7, alternate A=6.
        video(&mut dos, 0x1a01, 0x0108, 0, 0);
        {
            let mut context = dos.cpu().context();
            context.memory.write_u8(0x410, 0x30).unwrap(); // Monochrome description active.
            context.memory.write_u8(0x487, 0x40).unwrap(); // Guest metadata: 192 KiB, emulated cursor.
            context.memory.write_u8(0x489, 0x1f).unwrap(); // All four state flags set.

            // Direct guest register changes must be reflected, too.
            context.io.read_port(0x3da, 1);
            context.io.write_port(0x3c0, 1, 0x30);
            context.io.write_port(0x3c0, 1, 0x04); // Disable blink.
        }
        let bytes = information(&mut dos, 0x5000, 0x200, 0);
        assert_eq!(&bytes[0x25..0x27], &[1, 8]);
        assert_eq!(&bytes[0x2b..0x2e], &[7, 6, 0x1f]);
        assert_eq!(bytes[0x31], 2);
        assert_eq!(bytes[0x32], 0); // Different font banks are not a save-pointer override.
    });
}

#[test]
fn state_buffers_wrap_and_invalid_selectors_preserve_memory() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let expected = information(&mut dos, 0x5000, 0x200, 0);
        {
            let mut context = dos.cpu().context();
            context.memory.write(0x5fffb, &[0xa5; 5]).unwrap();
            context.memory.write(0x50000, &[0xa5; 61]).unwrap();
            context.memory.write(0x60000, &[0xa5; 64]).unwrap();
        }
        assert_eq!(information(&mut dos, 0x5000, 0xfffc, 0), expected);
        {
            let context = dos.cpu().context();
            assert_eq!(context.memory.read_u8(0x5fffb).unwrap(), 0xa5);
            assert_eq!(context.memory.read_u8(0x5003c).unwrap(), 0xa5);
            let mut following_segment = [0; 64];
            context
                .memory
                .read(0x60000, &mut following_segment)
                .unwrap();
            assert_eq!(following_segment, [0xa5; 64]);
        }
        dos.cpu()
            .context()
            .memory
            .write(0x50200, &[0xa5; 64])
            .unwrap();
        for selector in [1, 0xffff] {
            assert_eq!(information(&mut dos, 0x5000, 0x200, selector), [0xa5; 64]);
        }
        // Overlapping output must not replace BIOS inputs before they are read.
        assert_eq!(information(&mut dos, 0, 0x449, 0), expected);
    });
}
