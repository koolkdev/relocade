use crate::{
    common,
    information::information,
    video::{video, PROGRAM},
};
use relocade_x86::{Segment, SegmentRegister};

#[test]
fn palette_controls_change_only_bios_policy_and_report_invalid_selections() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        // Other profile/display bits must survive both controls.
        dos.cpu().context().memory.write_u8(0x489, 0x55).unwrap();
        let before = dos.vga().borrow().snapshot();
        let before_registers = dos.vga().borrow().registers().clone();
        for (function, selection, expected) in [
            (0x31, 1, 0x5d),
            (0x33, 0, 0x5f),
            (0x33, 1, 0x5d),
            (0x31, 0, 0x55),
        ] {
            let result = video(
                &mut dos,
                0x1200 | selection,
                0xa500 | function,
                0x5678,
                0x9abc,
            );
            assert_eq!(result.registers.eax as u16, 0x1212);
            assert_eq!(result.registers.ebx as u16, 0xa500 | function);
            assert_eq!(result.registers.ecx as u16, 0x5678);
            assert_eq!(result.registers.edx as u16, 0x9abc);
            assert_eq!(dos.cpu().context().memory.read_u8(0x489).unwrap(), expected);
            assert_eq!(dos.vga().borrow().registers().clone(), before_registers);
            let report = information(&mut dos, 0x5000, 0x200, 0);
            assert_eq!(report[0x2d] & 0x0f, expected & 0x0f);
        }
        for function in [0x31, 0x33] {
            for selection in [2, 255] {
                let result = video(&mut dos, 0x1200 | selection, function, 0, 0);
                assert_eq!(result.registers.eax as u16, 0x1200);
                assert_eq!(dos.cpu().context().memory.read_u8(0x489).unwrap(), 0x55);
            }
        }
        let after = dos.vga().borrow().snapshot();
        let after_registers = dos.vga().borrow().registers().clone();
        assert_eq!(after_registers, before_registers);
        assert!(after.shares_vram_with(&before));
    });
}

#[test]
fn mode_changes_preserve_or_convert_palettes_without_skipping_display_controls() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x1202, 0x30, 0, 0);
        video(&mut dos, 3, 0, 0, 0);
        let text = dos.vga().borrow().snapshot(); // Defaults for the later 400-line mode set.
        video(&mut dos, 0x1200, 0x30, 0, 0);
        video(&mut dos, 0x13, 0, 0, 0);
        let graphics = dos.vga().borrow().snapshot();
        // Mode loading must use the same conversion as the explicit service.
        video(&mut dos, 0x101b, 0, 256, 0);
        let gray_graphics = dos.vga().borrow().snapshot();
        {
            let context = dos.cpu().context();
            context.io.read_port(0x3da, 1);
            for index in 0..16 {
                context.io.write_port(0x3c0, 1, index);
                context.io.write_port(0x3c0, 1, 63 - index);
            }
            for (index, value) in [(0x10, 0), (0x11, 42), (0x12, 1), (0x13, 6), (0x14, 15)] {
                context.io.write_port(0x3c0, 1, index);
                context.io.write_port(0x3c0, 1, value);
            }
            context.io.write_port(0x3c0, 1, 0x20);
            context.io.write_port(0x3c8, 1, 7);
            for component in [5, 0, 41] {
                context.io.write_port(0x3c9, 1, component);
            }
            context.io.write_port(0x3c6, 1, 0x5a);
            context.io.write_port(0x3c7, 1, 42);
            context.io.read_port(0x3c9, 1); // Retain an unfinished DAC read too.
        }
        let custom = dos.vga().borrow().snapshot();
        let custom_registers = dos.vga().borrow().registers().clone();
        video(&mut dos, 0x1200, 0x33, 0, 0); // Grayscale enabled.
        video(&mut dos, 0x1201, 0x31, 0, 0); // Default palette disabled takes precedence.
        assert_eq!(dos.vga().borrow().registers().clone(), custom_registers);
        video(&mut dos, 0x1202, 0x30, 0, 0); // Scanline selection preserves the policies.
        for (mode, defaults) in [(0x83, &text), (0x93, &graphics)] {
            video(&mut dos, mode, 0, 0, 0);
            let capture = dos.vga().borrow().snapshot();
            let capture_registers = dos.vga().borrow().registers().clone();
            assert_eq!(
                &capture.vga().unwrap().attributes[..16],
                &custom.vga().unwrap().attributes[..16]
            );
            assert_eq!(capture.vga().unwrap().attributes[0x11], 42);
            assert_eq!(capture_registers.dac, custom_registers.dac);
            for register in [0x10, 0x12, 0x13, 0x14] {
                assert_eq!(
                    capture.vga().unwrap().attributes[register],
                    defaults.vga().unwrap().attributes[register]
                );
            }
            assert_ne!(capture_registers.attribute.address() & 0x20, 0);
            assert_eq!(dos.cpu().context().memory.read_u8(0x489).unwrap(), 0x1b);
        }
        // Disabling defaults does not block explicit BIOS palette writes.
        // High component bits are discarded before summing: [5, 0, 41] -> 6.
        video(&mut dos, 0x1010, 7, 0x4069, 0x8500);
        video(&mut dos, 0x1000, 0x2a02, 0, 0);
        let explicit = dos.vga().borrow().snapshot();
        let explicit_registers = dos.vga().borrow().registers().clone();
        assert_eq!(explicit.dac.colors[7], [6; 3]);
        assert_eq!(explicit.vga().unwrap().attributes[2], 42);
        video(&mut dos, 0x1200, 0x31, 0, 0);
        assert_eq!(dos.vga().borrow().registers().clone(), explicit_registers);
        video(&mut dos, 0x13, 0, 0, 0);
        let loaded = dos.vga().borrow().snapshot();
        let loaded_registers = dos.vga().borrow().registers().clone();
        assert_eq!(loaded.dac.colors, gray_graphics.dac.colors);
        assert_eq!(loaded.dac.mask, 255);
        assert_eq!(loaded_registers.dac.address(), 0);
        assert_eq!(
            loaded.vga().unwrap().attributes,
            graphics.vga().unwrap().attributes
        );
        video(&mut dos, 0x1201, 0x33, 0, 0);
        video(&mut dos, 3, 0, 0, 0);
        assert_eq!(dos.vga().borrow().snapshot().dac.colors, text.dac.colors);
    });
}

#[test]
fn grayscale_applies_to_bios_dac_buffers_but_not_direct_ports() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x1200, 0x33, 0, 0);
        video(&mut dos, 0x1201, 0x31, 0, 0);
        {
            let mut context = dos.cpu().context();
            context
                .cpu
                .set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0x5000));
            context.memory.write_u8(0x5ffff, 0x85).unwrap();
            context
                .memory
                .write(0x50000, &[0x40, 0x40, 0x40, 0x40, 0x69, 0x40, 0x72, 0x40])
                .unwrap();
        }
        video(&mut dos, 0x1012, 255, 3, 0xffff);
        let capture = dos.vga().borrow().snapshot();
        let capture_registers = dos.vga().borrow().registers().clone();
        assert_eq!(capture.dac.colors[255], [1; 3]);
        assert_eq!(capture.dac.colors[0], [5; 3]);
        assert_eq!(capture.dac.colors[1], [29; 3]);
        assert_eq!(capture_registers.dac.address(), 2);
        {
            let context = dos.cpu().context();
            context.io.write_port(0x3c8, 1, 7);
            for component in [63, 0, 0] {
                context.io.write_port(0x3c9, 1, component);
            }
            context.io.write_port(0x3c8, 1, 42);
            context.io.write_port(0x3c9, 1, 17); // Unfinished transfer survives an empty block.
        }
        let direct = dos.vga().borrow().snapshot();
        let direct_registers = dos.vga().borrow().registers().clone();
        assert_eq!(direct.dac.colors[7], [63, 0, 0]);
        video(&mut dos, 0x1012, 255, 0, 0xffff);
        assert_eq!(dos.vga().borrow().registers().clone(), direct_registers);
        video(&mut dos, 0x1201, 0x33, 0, 0);
        video(&mut dos, 0x1010, 7, 0, 0x3f00);
        assert_eq!(dos.vga().borrow().snapshot().dac.colors[7], [63, 0, 0]);
        // IBM's independent monochrome-display bit also enables BIOS summing.
        dos.cpu().context().memory.write_u8(0x489, 0x85).unwrap();
        video(&mut dos, 0x1010, 7, 0, 0x3f00);
        assert_eq!(dos.vga().borrow().snapshot().dac.colors[7], [19; 3]);
    });
}
