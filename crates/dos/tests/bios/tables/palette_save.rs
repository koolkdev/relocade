use super::{common, information, install, video, Dos, FarPointer, PRIMARY, PROGRAM};
use relocade_x86::{Segment, SegmentRegister};

const AREA: FarPointer = FarPointer {
    segment: 0x7000,
    offset: 0xfff8,
};

fn install_area(dos: &mut Dos) {
    install(dos);
    let mut context = dos.cpu().context();
    context
        .memory
        .write_wrapping(PRIMARY.wrapping_add(4), &AREA.packed().to_le_bytes())
        .unwrap();
    context.memory.write_wrapping(AREA, &[0xa5; 256]).unwrap();
}

fn saved(dos: &mut Dos) -> [u8; 256] {
    let mut bytes = [0; 256];
    dos.cpu()
        .context()
        .memory
        .read_wrapping(AREA, &mut bytes)
        .unwrap();
    bytes
}

#[test]
fn bios_palette_writes_update_only_the_seventeen_dynamic_entries() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        install_area(&mut dos);
        video(&mut dos, 3, 0, 0, 0);
        let initial = saved(&mut dos);
        assert_eq!(
            &initial[..17],
            &[0, 1, 2, 3, 4, 5, 0x14, 7, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0]
        );
        assert_eq!(&initial[17..], &[0xa5; 239]);
        assert_eq!(information(&mut dos, 0x4000, 0x200, 0)[0x32], 2);
        {
            let context = dos.cpu().context();
            context.io.read_port(0x3da, 1);
            context.io.write_port(0x3c0, 1, 4);
            context.io.write_port(0x3c0, 1, 0x2a);
            context
                .cpu
                .set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0x4000));
        }
        assert_eq!(saved(&mut dos), initial); // Direct I/O does not maintain BIOS RAM.
        video(&mut dos, 0x1c01, 0x300, 1, 0); // Save hardware only.
        video(&mut dos, 0x1000, 0xfe04, 0, 0);
        video(&mut dos, 0x1001, 0x1c00, 0, 0);
        let changed = saved(&mut dos);
        assert_eq!(changed[4], 0xfe); // The requested byte, before the hardware's six-bit mask.
        assert_eq!(changed[16], 0x1c);
        assert_eq!(&changed[17..], &[0xa5; 239]);
        assert_eq!(
            dos.vga().borrow().snapshot().registers.attribute.values[4],
            0x3e
        );
        video(&mut dos, 0x1c02, 0x300, 1, 0);
        assert_eq!(
            dos.vga().borrow().snapshot().registers.attribute.values[4],
            0x2a
        );
        assert_eq!(saved(&mut dos), changed); // Hardware restore does not restore firmware data.
        video(&mut dos, 0x1201, 0x31, 0, 0);
        video(&mut dos, 0x13, 0, 0, 0);
        assert_eq!(saved(&mut dos), changed); // Suppressed mode palette writes do not touch it.
        video(&mut dos, 0x1000, 0x5710, 0, 0); // Mode control is not a saved palette entry.
        assert_eq!(saved(&mut dos), changed);
    });
}

#[test]
fn legacy_and_block_palette_services_share_the_dynamic_save_area() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        install_area(&mut dos);
        video(&mut dos, 4, 0, 0, 0);
        video(&mut dos, 0x0b00, 0x0012, 0, 0);
        let background = saved(&mut dos);
        assert_eq!(&background[..4], &[2, 0x13, 0x15, 0x17]);
        assert_eq!(background[16], 2);
        video(&mut dos, 0x0b00, 0x0100, 0, 0);
        assert_eq!(&saved(&mut dos)[..4], &[2, 0x12, 0x14, 0x16]);
        let colors: [u8; 17] = std::array::from_fn(|index| 0x80 + index as u8);
        {
            let mut context = dos.cpu().context();
            context
                .cpu
                .set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0x8000));
            context
                .memory
                .write_wrapping(FarPointer::from_packed(0x8000_fffb), &colors)
                .unwrap();
        }
        video(&mut dos, 0x1002, 0, 0, 0xfffb);
        assert_eq!(&saved(&mut dos)[..17], &colors);
        assert_eq!(&saved(&mut dos)[17..], &[0xa5; 239]);
        assert_eq!(
            dos.vga().borrow().snapshot().registers.attribute.values[0x11],
            0x90
        );
    });
}
