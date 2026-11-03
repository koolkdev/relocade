//! Guest-installed BIOS tables affect the actual INT 10h services and VGA state.
#[path = "tables/fonts.rs"]
mod fonts;
#[path = "tables/palette_profile.rs"]
mod palette_profile;
#[path = "tables/palette_save.rs"]
mod palette_save;
use super::{
    information::information,
    video::{video, PROGRAM},
};
use crate::common;
use relocade_dos::Dos;
use relocade_machine::FarPointer;

const PRIMARY: FarPointer = FarPointer {
    segment: 0x5000,
    offset: 0xfff4,
};
const SECONDARY: FarPointer = FarPointer {
    segment: 0x5000,
    offset: 0x200,
};
const PARAMETERS: FarPointer = FarPointer {
    segment: 0x6000,
    offset: 0xffe0,
};

fn pointer(dos: &mut Dos, address: FarPointer) -> FarPointer {
    let mut bytes = [0; 4];
    dos.cpu()
        .context()
        .memory
        .read_wrapping(address, &mut bytes)
        .unwrap();
    FarPointer::from_packed(u32::from_le_bytes(bytes))
}

fn install(dos: &mut Dos) {
    let original = FarPointer::from_packed(dos.cpu().context().memory.read_u32(0x4a8).unwrap());
    let parameters = pointer(dos, original);
    let secondary = pointer(dos, original.wrapping_add(16));
    let mut context = dos.cpu().context();
    for (source, destination, size) in [
        (original, PRIMARY, 28),
        (secondary, SECONDARY, 26),
        (parameters, PARAMETERS, 29 * 64),
    ] {
        let mut bytes = vec![0; size];
        context.memory.read_wrapping(source, &mut bytes).unwrap();
        context.memory.write_wrapping(destination, &bytes).unwrap();
    }
    context
        .memory
        .write_wrapping(PRIMARY, &PARAMETERS.packed().to_le_bytes())
        .unwrap();
    context
        .memory
        .write_wrapping(PRIMARY.wrapping_add(16), &SECONDARY.packed().to_le_bytes())
        .unwrap();
    context.memory.write_u32(0x4a8, PRIMARY.packed()).unwrap();
}

#[test]
fn video_tables_publish_the_bios_layout_in_read_only_rom() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let root = FarPointer::from_packed(dos.cpu().context().memory.read_u32(0x4a8).unwrap());
        let parameters = pointer(&mut dos, root);
        let secondary = pointer(&mut dos, root.wrapping_add(16));
        let dcc = pointer(&mut dos, secondary.wrapping_add(2));
        let legacy = FarPointer::from_packed(dos.cpu().context().memory.read_u32(0x74).unwrap());
        let mut context = dos.cpu().context();
        assert_eq!(root.segment, 0xf000);
        let mut primary_bytes = [0xff; 28];
        context
            .memory
            .read_wrapping(root, &mut primary_bytes)
            .unwrap();
        assert_eq!(&primary_bytes[4..16], &[0; 12]);
        assert_eq!(&primary_bytes[20..], &[0; 8]);
        assert_eq!(context.memory.read_u16(secondary.address()).unwrap(), 26);
        let mut codes = [0; 36];
        context.memory.read_wrapping(dcc, &mut codes).unwrap();
        assert_eq!(&codes[..4], &[16, 1, 8, 0]);
        assert_eq!(&codes[26..28], &[0, 8]); // Default index 11: color VGA.
        assert_eq!(&codes[34..], &[6, 7]); // VGA mono + PGA.
        let mut record = [0; 64];
        context
            .memory
            .read_wrapping(parameters.wrapping_add(3 * 64), &mut record)
            .unwrap();
        assert_eq!(&record[..10], &[80, 24, 8, 0, 0x10, 1, 3, 0, 2, 0x43]);
        assert_eq!(record[10 + 0x13], 40); // CRTC Offset.
        assert_eq!(record[35 + 6], 0x14); // AC brown entry.
        assert_eq!(record[55 + 6], 0x0e); // B800 aperture.
        assert_ne!(legacy, parameters);
        let mut legacy_geometry = [0; 24];
        context
            .memory
            .read_wrapping(legacy.wrapping_add(64), &mut legacy_geometry)
            .unwrap();
        assert_eq!(&legacy_geometry[..8], &[0, 8, 0, 0x10, 0, 0x40, 0, 0x40]);
        assert_eq!(&legacy_geometry[8..16], &[40, 40, 80, 80, 40, 40, 80, 80]);
        context.memory.write_u8(parameters.address(), 0xa5).unwrap();
        assert_eq!(context.memory.read_u8(parameters.address()).unwrap(), 40);
    });
}

#[test]
fn mode_setting_consumes_replacement_parameters_including_cursor_and_display_start() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        install(&mut dos);
        {
            let mut context = dos.cpu().context();
            let record = PARAMETERS.wrapping_add(24 * 64); // Default 400-line mode 03h.
            let mut bytes = [0; 64];
            context.memory.read_wrapping(record, &mut bytes).unwrap();
            bytes[..5].copy_from_slice(&[60, 19, 10, 0, 0x18]);
            bytes[7] = 0x15; // Font B=5, A=1; independent of the built-in font choice.
            bytes[10 + 1] = 59;
            bytes[10 + 0x0a..10 + 0x10].copy_from_slice(&[2, 5, 0x12, 0x34, 0x23, 0x45]);
            bytes[10 + 0x13] = 30;
            bytes[35 + 2] = 0x12;
            context.memory.write_wrapping(record, &bytes).unwrap();
            context.memory.write_u16(0xb8000, 0x1f58).unwrap();
        }
        video(&mut dos, 0x0083, 0, 0, 0);
        assert_eq!(
            video(&mut dos, 0x0f00, 0, 0, 0).registers.eax as u16,
            0x3c83
        );
        let display = dos.vga().borrow().snapshot();
        assert_eq!(display.registers.sequencer.values[3], 0x15);
        assert_eq!(
            &display.registers.crtc.values[0x0a..0x10],
            &[2, 5, 0x12, 0x34, 0x23, 0x45]
        );
        assert_eq!(display.registers.crtc.values[0x13], 30);
        assert_eq!(display.registers.attribute.values[2], 0x12);
        let context = dos.cpu().context();
        assert_eq!(context.memory.read_u16(0x44c).unwrap(), 0x1800);
        assert_eq!(context.memory.read_u16(0x460).unwrap(), 0x0205);
        assert_eq!(context.memory.read_u8(0x484).unwrap(), 19);
        assert_eq!(context.memory.read_u16(0x485).unwrap(), 10);
        assert_eq!(context.memory.read_u16(0xb8000).unwrap(), 0x1f58);
    });
}

#[test]
fn parameter_record_selection_follows_vga_profiles_and_extended_mode_indices() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        install(&mut dos);
        for index in 0..29 {
            dos.cpu()
                .context()
                .memory
                .write_wrapping(PARAMETERS.wrapping_add(index * 64), &[40 + index as u8])
                .unwrap();
        }
        for (profile, mode, record) in [
            (0, 0, 0),
            (0, 3, 3),
            (1, 0, 19),
            (1, 1, 20),
            (1, 2, 21),
            (1, 3, 22),
            (2, 0, 23),
            (2, 1, 23),
            (2, 2, 24),
            (2, 3, 24),
            (0, 7, 7),
            (1, 7, 7),
            (2, 7, 25),
            (2, 4, 4),
            (2, 6, 6),
            (0, 0x0d, 13),
            (0, 0x0e, 14),
            (0, 0x0f, 17),
            (0, 0x10, 18),
            (0, 0x11, 26),
            (0, 0x12, 27),
            (0, 0x13, 28),
        ] {
            video(&mut dos, 0x1200 | profile, 0x30, 0, 0);
            video(&mut dos, 0x80 | mode, 0, 0, 0);
            assert_eq!(
                dos.cpu().context().memory.read_u16(0x44a).unwrap(),
                40 + record
            );
        }
    });
}

#[test]
fn display_combination_services_follow_the_live_secondary_table() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        install(&mut dos);
        {
            let mut context = dos.cpu().context();
            context
                .memory
                .write_wrapping(SECONDARY.wrapping_add(2), &0x7000_fffbu32.to_le_bytes())
                .unwrap();
            context
                .memory
                .write_wrapping(
                    FarPointer::from_packed(0x7000_fffb),
                    &[3, 1, 9, 0, 0, 0, 0, 9, 1, 8],
                )
                .unwrap();
        }
        video(&mut dos, 0x1a01, 0x0009, 0, 0);
        assert_eq!(dos.cpu().context().memory.read_u8(0x48a).unwrap(), 1);
        assert_eq!(video(&mut dos, 0x1a00, 0, 0, 0).registers.ebx as u16, 9);
        let report = information(&mut dos, 0x4000, 0x200, 0);
        assert_eq!(&report[0x25..0x27], &[9, 0]);
        assert_eq!(report[0x32], 0x20);
        // Mutating the pointed-to table affects the next call, without reinstalling it.
        dos.cpu().context().memory.write_u8(0x70002, 6).unwrap();
        assert_eq!(video(&mut dos, 0x1a00, 0, 0, 0).registers.ebx as u16, 6);
        video(&mut dos, 0x1a01, 0x0009, 0, 0);
        assert_eq!(
            video(&mut dos, 0x1a00, 0, 0, 0).registers.ebx as u16,
            0xffff
        );
    });
}
