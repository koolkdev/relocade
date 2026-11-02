use super::{buffer, state};
use crate::{
    common,
    video::{video, PROGRAM},
};

#[test]
fn full_restore_returns_to_programmed_text_after_a_graphics_mode_with_a_wrapped_buffer() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x1111, 0, 0, 0); // 14-high font and matching CRTC layout.
        video(&mut dos, 0x0200, 0x0200, 0, 0x0304);
        video(&mut dos, 0x0958, 0x022e, 10, 0);
        video(&mut dos, 0x0502, 0, 0, 0);
        video(&mut dos, 0x1010, 14, 0x071f, 0x3500);
        let original = common::next_capture(&dos).await;
        let expected_screen = common::screen(&dos);
        assert_eq!(
            state(&mut dos, 1, 7, 0x5000, 0xfffc).registers.eax as u16,
            0x1c1c
        );
        let saved = dos.vga().borrow().snapshot();
        let firmware = buffer::<0x42>(&mut dos, 0, 0x449);

        video(&mut dos, 0x93, 0, 0, 0); // Change interpretation, preserve pixels/fonts.
        let graphics = common::next_capture(&dos).await;
        assert_eq!(graphics.display.geometry().size, [320, 200]);
        assert_eq!(
            state(&mut dos, 2, 7, 0x5000, 0xfffc).registers.eax as u16,
            0x1c1c
        );
        let restored = common::next_capture(&dos).await;
        assert_eq!(
            restored.display.geometry().size,
            original.display.geometry().size
        );
        assert_eq!(common::screen(&dos), expected_screen);
        assert_eq!(restored.display.registers, saved.registers);
        assert_eq!(buffer::<0x42>(&mut dos, 0, 0x449), firmware);
        for plane in 0..4 {
            assert_eq!(restored.display.plane(plane), saved.plane(plane));
            assert_eq!(
                &original.display.plane(plane)[..0xffff],
                &saved.plane(plane)[..0xffff]
            );
        }
        assert_eq!(graphics.display.geometry().size, [320, 200]); // Retained capture.
    });
}

#[test]
fn hardware_restore_unlocks_monochrome_timing_and_reloads_latches_from_hostile_access_settings() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x12, 0, 0, 0);
        {
            let mut context = dos.cpu().context();
            for (plane, byte) in [0x96, 0x3c, 0xa5, 0x69].into_iter().enumerate() {
                context.io.write_port(0x3c4, 2, ((1 << plane) << 8) | 2);
                context.memory.write_u8(0xa0010, byte).unwrap();
            }
            context.memory.read_u8(0xa0010).unwrap();
            context.io.write_port(0x3c2, 1, 0xe0); // RAM disabled, monochrome ports.
            context.io.write_port(0x3c4, 2, 0x2101); // Save screen-off, too.
            context.io.write_port(0x3b4, 2, 0x8011); // Protected timing.
            context.io.write_port(0x3ba, 1, 0x5a);
            context.io.read_port(0x3ba, 1);
            context.io.write_port(0x3c0, 1, 0x17); // PAS clear and an unused index.
        }
        state(&mut dos, 1, 1, 0x5000, 0x200);
        let saved = dos.vga().borrow().snapshot();
        video(&mut dos, 0x13, 0, 0, 0);
        {
            let context = dos.cpu().context();
            context.io.write_port(0x3ce, 2, 0x0f01); // Set/reset all planes.
            context.io.write_port(0x3ce, 2, 0x1f03); // Rotate/XOR.
            context.io.write_port(0x3ce, 2, 0x1b05); // Compare reads, odd/even, mode 3.
            context.io.write_port(0x3ce, 2, 0x0008); // No writable bits.
            context.io.read_port(0x3da, 1);
            context.io.write_port(0x3c0, 1, 0x14);
            context.io.write_port(0x3c0, 1, 0x0b); // Color Select belongs to the DAC block.
            context.io.write_port(0x3c2, 1, 0x61); // Also disable RAM on entry.
        }
        let current = dos.vga().borrow().snapshot();
        let firmware = buffer::<0x42>(&mut dos, 0, 0x449);
        state(&mut dos, 2, 1, 0x5000, 0x200);
        let restored = dos.vga().borrow().snapshot();
        let mut expected = saved.registers.clone();
        expected.dac = current.registers.dac.clone(); // Hardware selection excludes DAC.
        expected.attribute.values[0x14] = current.registers.attribute.values[0x14];
        assert_eq!(restored.registers, expected);
        assert_eq!(buffer::<0x42>(&mut dos, 0, 0x449), firmware);
        for plane in 0..4 {
            assert_eq!(
                &restored.plane(plane)[..0xffff],
                &current.plane(plane)[..0xffff]
            );
        }
        {
            let mut context = dos.cpu().context();
            context.io.write_port(0x3c2, 1, 0xe2);
            context.io.write_port(0x3c4, 2, 0x0f02);
            context.io.write_port(0x3ce, 2, 0x0105);
            context.memory.write_u8(0xa0020, 0).unwrap();
        }
        let after_write = dos.vga().borrow().snapshot();
        for (plane, byte) in [0x96, 0x3c, 0xa5, 0x69].into_iter().enumerate() {
            assert_eq!(after_write.plane(plane)[0x20], byte);
        }
    });
}

#[test]
fn firmware_subset_restores_metadata_vectors_and_only_video_equipment_bits() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let addresses = [0x4a8, 0x14, 0x74, 0x7c, 0x10c];
        {
            let mut context = dos.cpu().context();
            context.memory.write_u8(0x410, 0x31).unwrap();
            for (index, address) in addresses.into_iter().enumerate() {
                context
                    .memory
                    .write_u32(address, 0x12345678 + index as u32)
                    .unwrap();
            }
        }
        let firmware = buffer::<0x42>(&mut dos, 0, 0x449);
        state(&mut dos, 1, 7, 0x5000, 0x200);
        video(&mut dos, 0x13, 0, 0, 0);
        {
            let mut context = dos.cpu().context();
            context.memory.write_u8(0x410, 0xc7).unwrap();
            for address in addresses {
                context.memory.write_u32(address, 0xdeadbeef).unwrap();
            }
        }
        let current = dos.vga().borrow().snapshot();
        state(&mut dos, 2, 2, 0x5000, 0x200);
        assert_eq!(buffer::<0x1e>(&mut dos, 0, 0x449), firmware[..0x1e]);
        assert_eq!(buffer::<7>(&mut dos, 0, 0x484), firmware[0x3b..0x42]);
        let context = dos.cpu().context();
        assert_eq!(context.memory.read_u8(0x410).unwrap(), 0xf7);
        for (index, address) in addresses.into_iter().enumerate() {
            assert_eq!(
                context.memory.read_u32(address).unwrap(),
                0x12345678 + index as u32
            );
        }
        let restored = dos.vga().borrow().snapshot();
        assert_eq!(restored.registers, current.registers);
        for plane in 0..4 {
            assert!(std::ptr::eq(restored.plane(plane), current.plane(plane)));
        }
    });
}

#[test]
fn dac_subset_restores_raw_colors_and_transfer_position_on_the_live_port_bank() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (port, index, mode) in [(0x3c7, 255u8, 1), (0x3c8, 42, 0)] {
            video(&mut dos, 3, 0, 0, 0);
            video(&mut dos, 0x1201, 0x33, 0, 0); // Disable grayscale for the saved colors.
            video(&mut dos, 0x1010, u16::from(index), 0x1531, 0x0700);
            video(&mut dos, 0x1000, 0x0d14, 0, 0);
            video(&mut dos, 0x1018, 0x5a, 0, 0);
            dos.cpu().context().io.write_port(port, 1, u32::from(index));
            state(&mut dos, 1, 7, 0x5000, 0x200);
            let saved = dos.vga().borrow().snapshot();
            video(&mut dos, 7, 0, 0, 0);
            video(&mut dos, 0x1200, 0x33, 0, 0); // Restore must bypass this policy.
            dos.cpu().context().memory.write_u16(0x463, 0x3d4).unwrap(); // Stale BDA.
            let firmware = buffer::<0x42>(&mut dos, 0, 0x449);
            let current = dos.vga().borrow().snapshot();
            state(&mut dos, 2, 4, 0x5000, 0x200);
            let restored = dos.vga().borrow().snapshot();
            let mut expected = current.registers.clone();
            expected.dac = saved.registers.dac.clone();
            expected.attribute.values[0x14] = 13;
            assert_eq!(restored.registers, expected);
            assert_eq!(
                restored.registers.dac.colors[usize::from(index)],
                [7, 21, 49]
            );
            assert_eq!(buffer::<0x42>(&mut dos, 0, 0x449), firmware);
            for plane in 0..4 {
                assert!(std::ptr::eq(restored.plane(plane), current.plane(plane)));
            }
            let context = dos.cpu().context();
            assert_eq!(context.io.read_port(0x3c7, 1) & 1, mode);
            if mode == 1 {
                assert_eq!(context.io.read_port(0x3c9, 1), 7);
            } else {
                context.io.write_port(0x3c9, 1, 62);
                assert_eq!(
                    dos.vga().borrow().snapshot().registers.dac.colors[42],
                    [62, 21, 49]
                );
            }
        }
    });
}

#[test]
fn relocated_blocks_are_collected_before_firmware_restore_overwrites_the_dac_source() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x1010, 0, 0x1234, 0x0700);
        state(&mut dos, 1, 6, 0x5000, 0x200);
        let firmware = buffer::<0x3a>(&mut dos, 0x5000, 0x220);
        let dac = buffer::<0x304>(&mut dos, 0x5000, 0x25a);
        let saved = dos.vga().borrow().snapshot();
        video(&mut dos, 0x1010, 0, 0, 0);
        {
            let mut context = dos.cpu().context();
            context.memory.write(0x1000, &firmware).unwrap();
            context.memory.write(0x449, &dac).unwrap();
            context
                .memory
                .write(0x900, &[0xff, 0xff, 0x00, 0x10, 0x49, 0x04])
                .unwrap();
        }
        state(&mut dos, 2, 6, 0, 0x900);
        assert_eq!(
            dos.vga().borrow().snapshot().registers.dac,
            saved.registers.dac
        );
        assert_eq!(buffer::<0x1e>(&mut dos, 0, 0x449), firmware[1..0x1f]);
        assert_eq!(buffer::<7>(&mut dos, 0, 0x484), firmware[0x1f..0x26]);
    });
}
