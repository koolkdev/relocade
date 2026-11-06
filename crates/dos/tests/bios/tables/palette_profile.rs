use super::{
    common, information, install,
    palette_save::{install_area, saved},
    video, Dos, FarPointer, PRIMARY, PROGRAM, SECONDARY,
};

const PROFILE: FarPointer = FarPointer {
    segment: 0x4000,
    offset: 0xfff0,
};

fn set_profile(dos: &mut Dos, bytes: &[u8; 22]) {
    let mut context = dos.cpu().context();
    context
        .memory
        .write_wrapping(SECONDARY.wrapping_add(10), &PROFILE.packed().to_le_bytes())
        .unwrap();
    context.memory.write_wrapping(PROFILE, bytes).unwrap();
}

#[test]
fn palette_profiles_apply_partial_ranges_grayscale_and_wrapping_sources() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        install_area(&mut dos);
        let mut profile = [
            0, 0, 0, 0, // Underline unchanged and reserved bytes.
            2, 0, 5, 0, 0xfe, 0xff, 0, 0x80, // Two AC entries at 5, source 8000:FFFE.
            2, 0, 255, 0, 0xfe, 0xff, 0, 0x90, // Two DAC colors at 255, source 9000:FFFE.
            3, 0xff,
        ];
        set_profile(&mut dos, &profile);
        {
            let mut context = dos.cpu().context();
            context
                .memory
                .write_wrapping(FarPointer::from_packed(0x8000_fffe), &[0x23, 0x34])
                .unwrap();
            context
                .memory
                .write_wrapping(FarPointer::from_packed(0x9000_fffe), &[63, 0, 0, 0, 63, 0])
                .unwrap();
        }
        video(&mut dos, 0x1200, 0x33, 0, 0); // Grayscale applies to the override too.
        video(&mut dos, 0x83, 0, 0, 0);
        let display = dos.vga().borrow().snapshot();
        assert_eq!(
            &display.vga().unwrap().attributes[4..8],
            &[4, 0x23, 0x34, 7]
        );
        assert_eq!(&display.vga().unwrap().attributes[0x10..0x12], &[0x0c, 0]);
        assert_eq!(display.dac.colors[255], [19; 3]);
        assert_eq!(display.dac.colors[0], [37; 3]);
        assert_eq!(&saved(&mut dos)[5..7], &[0x23, 0x34]);
        assert_eq!(saved(&mut dos)[16], 0);
        assert_eq!(&saved(&mut dos)[17..], &[0xa5; 239]);
        assert_eq!(information(&mut dos, 0x3000, 0x200, 0)[0x32], 0x12);

        profile[4] = 1;
        profile[6] = 16; // Logical entry 16 selects overscan, leaving AC10h alone.
        profile[12..16].copy_from_slice(&[0, 1, 3, 0]); // The full 256-color count uses its high byte.
        set_profile(&mut dos, &profile);
        let mut colors = [0; 256 * 3];
        colors[..3].copy_from_slice(&[1, 2, 3]);
        colors[255 * 3..].copy_from_slice(&[63, 0, 0]);
        dos.cpu()
            .context()
            .memory
            .write_wrapping(FarPointer::from_packed(0x9000_fffe), &colors)
            .unwrap();
        video(&mut dos, 3, 0, 0, 0);
        let display = dos.vga().borrow().snapshot();
        assert_eq!(
            &display.vga().unwrap().attributes[0x10..0x12],
            &[0x0c, 0x23]
        );
        assert_eq!(saved(&mut dos)[16], 0x23);
        assert_eq!(saved(&mut dos)[5], 5);
        assert_eq!(display.dac.colors[3], [2; 3]);
        assert_eq!(display.dac.colors[2], [19; 3]);
    });
}

#[test]
fn palette_suppression_still_applies_underline_after_font_overrides() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        install_area(&mut dos);
        {
            let mut context = dos.cpu().context();
            context
                .memory
                .write_wrapping(PRIMARY.wrapping_add(8), &0x5000_0400u32.to_le_bytes())
                .unwrap();
            context
                .memory
                .write(
                    0x50400,
                    &[10, 0, 1, 0, 65, 0, 0, 0x20, 0, 0x80, 0xff, 3, 0xff],
                )
                .unwrap();
            context.memory.write(0x82000, &[0x80; 10]).unwrap();
        }
        let mut profile = [
            1, 0, 0, 0, 17, 0, 0, 0, 0, 0, 0, 0x80, 0, 1, 0, 0, 0, 0, 0, 0x90, 3, 0xff,
        ];
        set_profile(&mut dos, &profile);
        video(&mut dos, 0x1010, 23, 0x0607, 0x0500);
        video(&mut dos, 0x1201, 0x31, 0, 0);
        let before = dos.vga().borrow().snapshot();
        let before_registers = dos.vga().borrow().registers().clone();
        video(&mut dos, 3, 0, 0, 0);
        let after = dos.vga().borrow().snapshot();
        let after_registers = dos.vga().borrow().registers().clone();
        assert_eq!(
            &after.vga().unwrap().attributes[..16],
            &before.vga().unwrap().attributes[..16]
        );
        assert_eq!(after_registers.dac, before_registers.dac);
        assert_eq!(saved(&mut dos), [0xa5; 256]);
        assert_eq!(after.vga().unwrap().crtc.values[0x14], 9); // Uses the overridden 10-high font.
        profile[0] = 0xff;
        set_profile(&mut dos, &profile);
        video(&mut dos, 3, 0, 0, 0);
        assert_eq!(
            dos.vga().borrow().snapshot().vga().unwrap().crtc.values[0x14],
            31
        );
        profile[0] = 1;
        set_profile(&mut dos, &profile);
        video(&mut dos, 1, 0, 0, 0); // Neither descriptor includes this mode.
        assert_eq!(
            dos.vga().borrow().snapshot().vga().unwrap().crtc.values[0x14],
            31
        );
        assert_eq!(information(&mut dos, 0x3000, 0x200, 0)[0x32], 0x16);
        profile[20] = 0x13;
        set_profile(&mut dos, &profile);
        video(&mut dos, 0x13, 0, 0, 0);
        // IBM writes the whole register, replacing mode 13h's doubleword bit too.
        assert_eq!(
            dos.vga().borrow().snapshot().vga().unwrap().crtc.values[0x14],
            7
        );
    });
}

#[test]
fn invalid_palette_ranges_do_not_write_unrelated_registers() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        install(&mut dos);
        let before_registers = dos.vga().borrow().registers().clone();
        set_profile(
            &mut dos,
            &[
                0, 0, 0, 0, 2, 0, 16, 0, 0, 0, 0,
                0x80, // Extends past the 17 logical entries.
                1, 1, 0, 0, 0, 0, 0, 0x90, // 257 colors exceeds the descriptor limit.
                3, 0xff,
            ],
        );
        video(&mut dos, 3, 0, 0, 0);
        assert_eq!(dos.vga().borrow().registers().clone(), before_registers);
        set_profile(
            &mut dos,
            &[
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 3, 0xff,
            ],
        );
        video(&mut dos, 3, 0, 0, 0); // Empty descriptors leave DAC addressing alone.
        assert_eq!(dos.vga().borrow().registers().clone(), before_registers);
    });
}
