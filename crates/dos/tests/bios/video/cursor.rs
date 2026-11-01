use super::{video, PROGRAM};
use crate::{common, information::information};
use relocade_dos::Dos;

fn assert_shape(dos: &mut Dos, requested: u16, hardware: u16) {
    video(dos, 0x0100, 0, requested, 0);
    assert_eq!(
        dos.cpu().context().memory.read_u16(0x460).unwrap(),
        requested
    );
    assert_eq!(video(dos, 0x0300, 0, 0, 0).registers.ecx as u16, requested);
    let snapshot = dos.vga().borrow().snapshot();
    assert_eq!(
        &snapshot.registers.crtc.values[0x0a..0x0c],
        &hardware.to_be_bytes(),
        "requested shape {requested:04x}"
    );
}

#[test]
fn cursor_policy_preserves_raw_default_and_applies_only_to_subsequent_shape_writes() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        assert_eq!(dos.cpu().context().memory.read_u8(0x487).unwrap(), 0x61);
        video(&mut dos, 0x1202, 0x30, 0, 0);
        video(&mut dos, 3, 0, 0, 0); // Sixteen-high text makes translation visible.
        assert_shape(&mut dos, 0x0607, 0x0607);
        assert_shape(&mut dos, 0x2007, 0x2007);
        dos.cpu().context().memory.write_u8(0x487, 0xf7).unwrap();

        for (selection, control, emulation_flag) in [(0, 0xf6, 0x10), (1, 0xf7, 0)] {
            let before = dos.vga().borrow().snapshot();
            let result = video(&mut dos, 0x1200 | selection, 0xa534, 0x5678, 0x9abc);
            assert_eq!(result.registers.eax as u16, 0x1212);
            assert_eq!(result.registers.ebx as u16, 0xa534);
            assert_eq!(result.registers.ecx as u16, 0x5678);
            assert_eq!(result.registers.edx as u16, 0x9abc);
            assert_eq!(dos.cpu().context().memory.read_u8(0x487).unwrap(), control);
            let after = dos.vga().borrow().snapshot();
            assert_eq!(after.registers, before.registers);
            for plane in 0..4 {
                assert!(std::ptr::eq(after.plane(plane), before.plane(plane)));
            }
            assert_eq!(
                information(&mut dos, 0x5000, 0x200, 0)[0x2d] & 0x10,
                emulation_flag
            );
            assert_shape(
                &mut dos,
                0x0607,
                if selection == 0 { 0x0d0e } else { 0x0607 },
            );
        }

        video(&mut dos, 0x1200, 0x34, 0, 0);
        let before = dos.vga().borrow().snapshot();
        for selection in [2, 255] {
            assert_eq!(
                video(&mut dos, 0x1200 | selection, 0x34, 0, 0)
                    .registers
                    .eax as u16,
                0x1200
            );
            assert_eq!(dos.cpu().context().memory.read_u8(0x487).unwrap(), 0xf6);
            assert_eq!(dos.vga().borrow().snapshot().registers, before.registers);
        }
        for mode in [0x83, 7, 0x13, 3] {
            video(&mut dos, mode, 0, 0, 0);
            assert_eq!(
                dos.cpu().context().memory.read_u8(0x487).unwrap() & 0x7f,
                0x76
            );
        }
    });
}

#[test]
fn emulated_shapes_follow_ibm_font_height_range_and_control_bit_rules() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x1200, 0x34, 0, 0);
        for (profile, height, underline) in [(0, 8, 0x0607), (1, 14, 0x0b0c), (2, 16, 0x0d0e)] {
            video(&mut dos, 0x1200 | profile, 0x30, 0, 0);
            video(&mut dos, 3, 0, 0, 0);
            assert_eq!(dos.cpu().context().memory.read_u16(0x485).unwrap(), height);
            assert_shape(&mut dos, 0x0607, underline);
        }
        for (requested, hardware) in [
            (0x0007, 0x000f), // Full block.
            (0x0307, 0x080f), // Half block.
            (0x0507, 0x0c0e), // Three-line bottom cursor.
            (0x0703, 0x030f), // Reversed range.
            (0x0700, 0x0700), // Empty reversed range.
            (0x050f, 0x050f), // Native bottom range.
            (0x0e0e, 0x0e0e), // Native penultimate line.
            (0x0203, 0x0203), // Top bar.
            (0x2007, 0x1e00), // Legacy hide flag.
            (0x4007, 0x0007), // Other start flags bypass scaling; CRTC masks bit 6.
            (0x6007, 0x2007),
            (0x0627, 0x0627), // Cursor skew bypasses scaling.
        ] {
            assert_shape(&mut dos, requested, hardware);
        }
        video(&mut dos, 0x1201, 0x30, 0, 0);
        video(&mut dos, 3, 0, 0, 0);
        // IBM's (start | end) range test rejects this otherwise native-bottom shape.
        assert_shape(&mut dos, 0x060d, 0x070d);
        video(&mut dos, 7, 0, 0, 0);
        assert_shape(&mut dos, 0x0607, 0x0b0c); // The monochrome CRTC receives translation too.

        // Guest firmware data can inhibit emulation or supply a nonstandard height.
        dos.cpu().context().memory.write_u8(0x487, 0x68).unwrap();
        assert_shape(&mut dos, 0x0607, 0x0607);
        assert_shape(&mut dos, 0x2007, 0x2007);
        dos.cpu().context().memory.write_u8(0x487, 0x60).unwrap();
        for height in [0, 128] {
            dos.cpu().context().memory.write_u16(0x485, height).unwrap();
            assert_shape(&mut dos, 0x0607, 0x3e7f); // Byte wrapping, signed height comparison.
        }
    });
}

#[test]
fn font_layout_keeps_its_native_cursor_when_emulation_is_enabled() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x1202, 0x30, 0, 0);
        video(&mut dos, 3, 0, 0, 0);
        video(&mut dos, 0x1200, 0x34, 0, 0);
        for (font, native, emulated) in [
            (0x1111, 0x0c0du16, 0x0b0c),
            (0x1112, 0x0607, 0x0607),
            (0x1114, 0x0e0f, 0x0d0e),
        ] {
            video(&mut dos, font, 0, 0, 0);
            assert_eq!(dos.cpu().context().memory.read_u16(0x460).unwrap(), native);
            assert_eq!(
                &dos.vga().borrow().snapshot().registers.crtc.values[0x0a..0x0c],
                &native.to_be_bytes()
            );
            assert_shape(&mut dos, 0x0607, emulated);
        }
        // A zero-glyph custom load still applies the requested fifteen-high layout.
        video(&mut dos, 0x1110, 0x0f00, 0, 0);
        assert_eq!(dos.cpu().context().memory.read_u16(0x485).unwrap(), 15);
        assert_eq!(dos.cpu().context().memory.read_u16(0x460).unwrap(), 0x0d0e);
        assert_eq!(
            &dos.vga().borrow().snapshot().registers.crtc.values[0x0a..0x0c],
            &[13, 14]
        );
        // The same shape supplied through AH=01h must still use IBM's translation.
        assert_shape(&mut dos, 0x0d0e, 0x0c0d);
    });
}
