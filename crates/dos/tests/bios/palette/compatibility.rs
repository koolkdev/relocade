use super::{common, video, PROGRAM};

#[test]
fn legacy_color_calls_program_vga_attributes_and_preserve_memory_and_port_selection() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 4, 0, 0, 0);
        {
            let context = dos.cpu().context();
            context.io.read_port(0x3da, 1);
            context.io.write_port(0x3c0, 1, 5); // Keep display disabled, register five selected.
        }
        let before = dos.vga().borrow().snapshot();
        let before_registers = dos.vga().borrow().registers().clone();
        assert_eq!(
            &before.vga().unwrap().attributes[..4],
            &[0, 0x13, 0x15, 0x17]
        );
        for (bx, shadow, colors, overscan) in [
            (0x0009, 0x29, [0x11, 3, 5, 7], 0x11),
            (0x0100, 0x09, [0x11, 2, 4, 6], 0x11),
            (0x001e, 0x1e, [0x16, 0x12, 0x14, 0x16], 0x16),
            (0x0103, 0x3e, [0x16, 0x13, 0x15, 0x17], 0x16),
        ] {
            video(&mut dos, 0x0b00, bx, 0, 0);
            assert_eq!(dos.cpu().context().memory.read_u8(0x466).unwrap(), shadow);
            let after = dos.vga().borrow().snapshot();
            let after_registers = dos.vga().borrow().registers().clone();
            let mut expected = before_registers.clone();
            expected.attribute.values[..4].copy_from_slice(&colors);
            expected.attribute.values[0x11] = overscan;
            assert_eq!(after_registers, expected);
            assert!(before.shares_vram_with(&after));
        }

        // Mode 06h uses the VGA palette service too, not a CGA-only foreground rule.
        video(&mut dos, 6, 0, 0, 0);
        video(&mut dos, 0x0b00, 2, 0, 0);
        let capture = dos.vga().borrow().snapshot();
        assert_eq!(&capture.vga().unwrap().attributes[..4], &[2, 3, 5, 7]);
        assert_eq!(capture.vga().unwrap().attributes[0x11], 2);
        assert_eq!(dos.cpu().context().memory.read_u8(0x466).unwrap(), 0x22);
    });
}

#[test]
fn legacy_color_shadows_follow_mode_sets_and_text_palette_selection_is_a_no_op() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 6, 0, 0, 0);
        video(&mut dos, 0x0d, 0, 0, 0);
        assert_eq!(dos.cpu().context().memory.read_u16(0x465).unwrap(), 0x3f1e);
        video(&mut dos, 3, 0, 0, 0);
        assert_eq!(dos.cpu().context().memory.read_u16(0x465).unwrap(), 0x3029);
        let before_registers = dos.vga().borrow().registers().clone();
        video(&mut dos, 0x0b00, 0x0100, 0, 0);
        assert_eq!(dos.cpu().context().memory.read_u8(0x466).unwrap(), 0x30);
        assert_eq!(dos.vga().borrow().registers().clone(), before_registers);

        // Background requests affect text overscan but leave text palette entries alone.
        video(&mut dos, 0x0b00, 0x001e, 0, 0);
        let mut expected = before_registers;
        expected.attribute.values[0x11] = 0x16;
        assert_eq!(dos.vga().borrow().registers().clone(), expected);
        assert_eq!(dos.cpu().context().memory.read_u8(0x466).unwrap(), 0x3e);
        assert_eq!(
            video(&mut dos, 0x0b00, 0x0200, 0, 0).registers.eax as u16,
            0x8600
        );
        assert_eq!(dos.vga().borrow().registers().clone(), expected);
        assert_eq!(dos.cpu().context().memory.read_u8(0x466).unwrap(), 0x3e);

        // Monochrome routing ignores even unsupported selectors.
        video(&mut dos, 7, 0, 0, 0);
        let before_registers = dos.vga().borrow().registers().clone();
        for bx in [0x000f, 0x0100, 0x0200] {
            let result = video(&mut dos, 0x0b00, bx, 0xa5c3, 0x1234);
            assert_eq!(result.registers.eax as u16, 0x0b00);
            assert_eq!(result.registers.ebx as u16, bx);
            assert_eq!(result.registers.ecx as u16, 0xa5c3);
            assert_eq!(result.registers.edx as u16, 0x1234);
            assert_eq!(dos.cpu().context().memory.read_u8(0x466).unwrap(), 0x30);
            assert_eq!(dos.vga().borrow().registers().clone(), before_registers);
        }
    });
}
