use super::*;

fn shifts(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    // opcode, immediate opcode/extension, lane width, input, shifts by 1 and width-1.
    for (opcode, immediate, extension, width, input, one, last, oversized) in [
        (
            0xf1,
            0x71,
            6,
            16,
            0x8001_7fff_ffff_0001,
            0x0002_fffe_fffe_0002,
            0x8000_8000_8000_8000,
            0,
        ),
        (
            0xf2,
            0x72,
            6,
            32,
            0x8000_0001_7fff_ffff,
            0x0000_0002_ffff_fffe,
            0x8000_0000_8000_0000,
            0,
        ),
        (
            0xf3,
            0x73,
            6,
            64,
            0x8000_0000_0000_0001,
            2,
            0x8000_0000_0000_0000,
            0,
        ),
        (
            0xd1,
            0x71,
            2,
            16,
            0x8001_7fff_ffff_0001,
            0x4000_3fff_7fff_0000,
            0x0001_0000_0001_0000,
            0,
        ),
        (
            0xd2,
            0x72,
            2,
            32,
            0x8000_0001_7fff_ffff,
            0x4000_0000_3fff_ffff,
            0x0000_0001_0000_0000,
            0,
        ),
        (
            0xd3,
            0x73,
            2,
            64,
            0x8000_0000_0000_0001,
            0x4000_0000_0000_0000,
            1,
            0,
        ),
        (
            0xe1,
            0x71,
            4,
            16,
            0x8001_7fff_ffff_0001,
            0xc000_3fff_ffff_0000,
            0xffff_0000_ffff_0000,
            0xffff_0000_ffff_0000,
        ),
        (
            0xe2,
            0x72,
            4,
            32,
            0x8000_0001_7fff_ffff,
            0xc000_0000_3fff_ffff,
            0xffff_ffff_0000_0000,
            0xffff_ffff_0000_0000,
        ),
    ] {
        for (count, result) in [
            (0, input),
            (1, one),
            (width - 1, last),
            (width, oversized),
            (width + 1, oversized),
            (255, oversized),
            (0x1_0000_0000, oversized),
            (0x1_0000_0001, oversized),
            (u64::MAX, oversized),
        ] {
            check_binary(&mut checks, opcode, input, count, result);
            if count <= 255 {
                let code = [0x0f, immediate, 0xc0 | extension << 3 | 7, count as u8];
                let mut image = image(&code);
                write_register_bits(&mut image.cpu, 7, (input, 0x1234));
                let mut cpu = complete(image.cpu, 4);
                write_register_bits(&mut cpu, 7, (result, 0xffff));
                checks.check(
                    "immediate shift and MMX effects, including count zero",
                    &code,
                    &image,
                    &[dispatch(cpu)],
                );
            }
        }
    }
    let code = [0x0f, 0xf1, 0xff]; // PSLLW mm7, mm7: snapshot both operands before writing.
    let mut image = image(&code);
    write_register_bits(&mut image.cpu, 7, (1, 0x1234));
    let mut cpu = complete(image.cpu, 3);
    write_register_bits(&mut cpu, 7, (2, 0xffff));
    checks.check(
        "destination is also the count",
        &code,
        &image,
        &[dispatch(cpu)],
    );
}

fn shift_faults(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let code = [0x0f, 0xf1, 0x38]; // Even a zero destination must read all eight count bytes.
    let mut image = image(&code);
    write_register_bits(&mut image.cpu, 7, (0, 0x1234));
    image.cpu.registers.eax = 0x4ffc;
    image.map(4, 0x8000, false);
    image.data(0x8ffc, &[0; 4]);
    checks.check(
        "count read faults before any MMX effect",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::PageFault {
                address: 0x5000,
                error: 0,
            },
        }],
    );
    image.cpu.x87.status.error_summary = 1;
    checks.check(
        "pending exception precedes the count read",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

#[test]
fn mmx_immediate_shifts_reject_memory_destinations_before_reading_more_bytes() {
    use crate::support::step::TestModule;
    use wasm86_x86::{compile_block_from_bytes, BlockError};
    for code in [[0x0f, 0x71, 0x34], [0x0f, 0x72, 0x24], [0x0f, 0x73, 0x14]] {
        assert_eq!(
            compile_block_from_bytes(0x1ffd, &code, 1).err(),
            Some(BlockError::UnsupportedInstruction {
                address: 0x1ffd,
                opcode: 0x0f
            })
        );
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x1ffd;
        image.data(0x3ffd, &code);
        image.check_unchanged_exit(
            TestModule::interpreter(),
            "memory form is rejected before missing SIB or immediate bytes",
            Exit::Other(0x0008_000f_0000_1ffd),
        );
    }
}

test_frontends!(mmx_packed_shifts, shifts);
test_frontends!(mmx_shift_count_faults, shift_faults);
