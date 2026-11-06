use super::{information, video, PROGRAM};
use crate::common;
use relocade_dos::Dos;
use relocade_x86::CpuState;

fn request(dos: &mut Dos, function: u8, bx: u16, cx: u16, dx: u16) -> CpuState {
    {
        let context = dos.cpu().context();
        context.cpu.set_si(0x1234);
        context.cpu.set_di(0x5678);
        context.cpu.set_bp(0x9abc);
    }
    let result = video(dos, 0x4f00 | u16::from(function), bx, cx, dx);
    assert_eq!(result.registers.esi as u16, 0x1234);
    assert_eq!(result.registers.edi as u16, 0x5678);
    assert_eq!(result.registers.ebp as u16, 0x9abc);
    result
}

fn scanline(dos: &mut Dos, operation: u16, width: u16, expected: [u16; 4]) {
    let result = request(dos, 6, operation, width, 0xabcd).registers;
    assert_eq!(
        [result.eax, result.ebx, result.ecx, result.edx].map(|value| value as u16),
        expected
    );
}

#[test]
fn logical_scanlines_report_actual_pitch_and_mode_information() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        scanline(&mut dos, 1, 0, [0x14f, 1, 0, 0xabcd]); // Legacy mode is unsupported.
        video(&mut dos, 0x4f02, 0x101, 0, 0);
        scanline(&mut dos, 1, 0, [0x4f, 640, 640, 6553]);
        scanline(&mut dos, 0x5500, 1024, [0x4f, 1024, 1024, 4096]); // Only BL selects.
        let (status, bytes) = information(&mut dos, 1, 0x101, 0x200);
        assert_eq!(status, 0x4f);
        assert_eq!(&bytes[16..22], &[0, 4, 0x80, 2, 0xe0, 1]); // Pitch changes, size does not.
        let (_, other) = information(&mut dos, 1, 0x103, 0x200);
        assert_eq!(&other[16..18], &[0x20, 3]); // Inactive mode has its default pitch.
        scanline(&mut dos, 2, 1280, [0x4f, 1280, 1280, 3276]);
        scanline(&mut dos, 3, 0xffff, [0x4f, 8738, 8738, 480]);
        scanline(&mut dos, 1, 0, [0x4f, 1280, 1280, 3276]); // Maximum query has no effect.
        scanline(&mut dos, 0, 639, [0x4f, 640, 640, 6553]); // Round up to visible width.
    });
}

#[test]
fn rejected_scanlines_preserve_viewport_banking_and_memory() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x4f02, 0x101, 0, 0);
        scanline(&mut dos, 0, 1024, [0x4f, 1024, 1024, 4096]);
        {
            let mut context = dos.cpu().context();
            for (register, value) in [(5, 63), (8, 32), (9, 10)] {
                context.io.write_port(0x1ce, 2, register);
                context.io.write_port(0x1cf, 2, value);
            }
            context.memory.write_u8(0xaffff, 0xa5).unwrap();
        }
        scanline(&mut dos, 0, 640, [0x14f, 0, 640, 0xabcd]); // Origin would cross a row.
        scanline(&mut dos, 2, 8739, [0x24f, 2, 8739, 0xabcd]); // Too wide for 480 rows.
        scanline(&mut dos, 0x80, 1024, [0x14f, 0x80, 1024, 0xabcd]);
        scanline(&mut dos, 3, 0, [0x4f, 8738, 8738, 480]); // Capability ignores origin.
        scanline(&mut dos, 1, 0, [0x4f, 1024, 1024, 4096]);
        scanline(&mut dos, 0, 2048, [0x4f, 2048, 2048, 2048]);
        let context = dos.cpu().context();
        for (register, expected) in [(5, 63), (8, 32), (9, 10)] {
            context.io.write_port(0x1ce, 2, register);
            assert_eq!(context.io.read_port(0x1cf, 2), expected);
        }
        assert_eq!(context.memory.read_u8(0xaffff).unwrap(), 0xa5);
    });
}
