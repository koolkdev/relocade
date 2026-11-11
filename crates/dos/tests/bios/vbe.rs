#[path = "vbe/layout.rs"]
mod layout;
#[path = "vbe/modes.rs"]
mod modes;
#[path = "vbe/palette.rs"]
mod palette;
#[path = "vbe/state.rs"]
mod state;

use super::video::{video, PROGRAM};
use crate::common;
use relocade_dos::Dos;
use relocade_machine::FarPointer;
use relocade_x86::{Segment, SegmentRegister};

fn information(dos: &mut Dos, function: u8, mode: u16, offset: u16) -> (u16, [u8; 256]) {
    let pointer = FarPointer {
        segment: 0x5000,
        offset,
    };
    {
        let mut context = dos.cpu().context();
        context.cpu.set_segment(
            Segment::Es,
            SegmentRegister::real_mode(Segment::Es, pointer.segment),
        );
        context.cpu.set_di(offset);
        context.cpu.set_si(0x1234);
        context.cpu.set_bp(0x5678);
        context
            .memory
            .write_wrapping(pointer.wrapping_add(0xffff), &[0xa5; 258])
            .unwrap();
        context.memory.write_wrapping(pointer, b"VBE2").unwrap();
        context.io.write_port(0x1ce, 2, 0x104);
    }
    let result = video(dos, 0x4f00 | u16::from(function), 0x9876, mode, 0xabcd);
    assert_eq!(result.registers.ebx as u16, 0x9876);
    assert_eq!(result.registers.ecx as u16, mode);
    assert_eq!(result.registers.edx as u16, 0xabcd);
    assert_eq!(result.registers.esi as u16, 0x1234);
    assert_eq!(result.registers.ebp as u16, 0x5678);
    assert_eq!(result.registers.edi as u16, offset);
    assert_eq!(result.segments.es.selector, pointer.segment);
    let context = dos.cpu().context();
    assert_eq!(context.io.read_port(0x1ce, 2), 0x104);
    assert_eq!(
        context
            .memory
            .read_u8(pointer.wrapping_add(0xffff).address())
            .unwrap(),
        0xa5
    );
    assert_eq!(
        context
            .memory
            .read_u8(pointer.wrapping_add(256).address())
            .unwrap(),
        0xa5
    );
    let mut bytes = [0; 256];
    context.memory.read_wrapping(pointer, &mut bytes).unwrap();
    (result.registers.eax as u16, bytes)
}

#[test]
fn vbe_discovery_returns_bounded_structures_and_valid_rom_pointers() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let registers = dos.vga().borrow().registers();
        let (status, bytes) = information(&mut dos, 0, 0, 0xff80);
        assert_eq!(status, 0x004f);
        assert_eq!(&bytes[..6], b"VESA\x02\x01");
        assert_eq!(&bytes[10..14], &[0; 4]);
        assert_eq!(&bytes[18..20], &[64, 0]); // Four MiB in 64-KiB blocks.
        assert_eq!(&bytes[20..], &[0; 236]);
        let oem = FarPointer::from_packed(u32::from_le_bytes(bytes[6..10].try_into().unwrap()));
        let modes = FarPointer::from_packed(u32::from_le_bytes(bytes[14..18].try_into().unwrap()));
        let context = dos.cpu().context();
        let mut name = [0; 21];
        context.memory.read_wrapping(oem, &mut name).unwrap();
        assert_eq!(&name, b"Relocade banked SVGA\0");
        let mut list = [0; 20];
        context.memory.read_wrapping(modes, &mut list).unwrap();
        assert_eq!(
            list.as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_le_bytes)
                .collect::<Vec<_>>(),
            [0x101, 0x103, 0x105, 0x111, 0x114, 0x117, 0x142, 0x143, 0x144, 0xffff]
        );
        assert_eq!(dos.vga().borrow().registers(), registers);
    });
}

#[test]
fn vbe_mode_information_describes_only_the_supported_banked_layout() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let (status, bytes) = information(&mut dos, 1, 0x101, 0x200);
        assert_eq!(status, 0x004f);
        assert_eq!(
            &bytes[..16],
            &[0x1b, 0, 7, 0, 64, 0, 64, 0, 0, 0xa0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            &bytes[16..31],
            &[0x80, 2, 0x80, 2, 0xe0, 1, 8, 16, 1, 8, 1, 4, 0, 12, 1]
        );
        assert_eq!(&bytes[31..], &[0; 225]);
        for (function, mode) in [
            (1, 0x13),
            (1, 0x107),
            (1, 0x4101),
            (1, 0x8101),
            (0xff, 0x101),
        ] {
            let (status, bytes) = information(&mut dos, function, mode, 0x200);
            assert_eq!(status, if function == 1 { 0x014f } else { 0x0100 });
            assert_eq!(&bytes[..4], b"VBE2");
            assert_eq!(&bytes[4..], &[0xa5; 252]);
        }
    });
}

#[test]
fn advertised_resolutions_match_mode_information_and_current_mode() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (number, width, height, bits, pages) in [
            (0x101, 640u16, 480u16, 8u8, 12),
            (0x103, 800, 600, 8, 7),
            (0x105, 1024, 768, 8, 4),
            (0x111, 640, 480, 16, 5),
            (0x114, 800, 600, 16, 3),
            (0x117, 1024, 768, 16, 1),
            (0x142, 640, 480, 32, 2),
            (0x143, 800, 600, 32, 1),
            (0x144, 1024, 768, 32, 0),
        ] {
            let (status, bytes) = information(&mut dos, 1, number, 0x200);
            assert_eq!(status, 0x4f);
            assert_eq!(&bytes[16..18], &(width * u16::from(bits / 8)).to_le_bytes());
            assert_eq!(&bytes[18..20], &width.to_le_bytes());
            assert_eq!(&bytes[20..22], &height.to_le_bytes());
            assert_eq!(bytes[25], bits);
            assert_eq!(bytes[27], if bits == 8 { 4 } else { 6 });
            assert_eq!(bytes[29], pages);
            let masks = match bits {
                8 => [0; 9],
                16 => [5, 11, 6, 5, 5, 0, 0, 0, 0],
                32 => [8, 16, 8, 8, 8, 0, 8, 24, 2],
                _ => unreachable!(),
            };
            assert_eq!(bytes[31..40], masks);
            assert_eq!(&bytes[40..], &[0; 216]); // No linear framebuffer.
            assert_eq!(
                video(&mut dos, 0x4f02, number, 0, 0).registers.eax as u16,
                0x4f
            );
            assert_eq!(
                video(&mut dos, 0x4f03, 0, 0, 0).registers.ebx as u16,
                number
            );
        }
    });
}
