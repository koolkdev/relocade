use super::*;

fn shifts(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // Each half has different signs and low bits; a count never wraps at the lane width.
    for (opcode, immediate, extension, width, input, one, last, oversized) in [
        (
            0xf1,
            0x71,
            6,
            16,
            0x000080007ffefffe_80017fffffff0001,
            0x00000000fffcfffc_0002fffefffe0002,
            0x0000000000000000_8000800080008000,
            0,
        ),
        (
            0xf2,
            0x72,
            6,
            32,
            0x00000002ffffffff_800000017fffffff,
            0x00000004fffffffe_00000002fffffffe,
            0x0000000080000000_8000000080000000,
            0,
        ),
        (
            0xf3,
            0x73,
            6,
            64,
            0x7ffffffffffffffe_8000000000000001,
            0xfffffffffffffffc_0000000000000002,
            0x0000000000000000_8000000000000000,
            0,
        ),
        (
            0xd1,
            0x71,
            2,
            16,
            0x000080007ffefffe_80017fffffff0001,
            0x000040003fff7fff_40003fff7fff0000,
            0x0000000100000001_0001000000010000,
            0,
        ),
        (
            0xd2,
            0x72,
            2,
            32,
            0x00000002ffffffff_800000017fffffff,
            0x000000017fffffff_400000003fffffff,
            0x0000000000000001_0000000100000000,
            0,
        ),
        (
            0xd3,
            0x73,
            2,
            64,
            0x7ffffffffffffffe_8000000000000001,
            0x3fffffffffffffff_4000000000000000,
            0x0000000000000000_0000000000000001,
            0,
        ),
        (
            0xe1,
            0x71,
            4,
            16,
            0x000080007ffefffe_80017fffffff0001,
            0x0000c0003fffffff_c0003fffffff0000,
            0x0000ffff0000ffff_ffff0000ffff0000,
            0x0000ffff0000ffff_ffff0000ffff0000,
        ),
        (
            0xe2,
            0x72,
            4,
            32,
            0x00000002ffffffff_800000017fffffff,
            0x00000001ffffffff_c00000003fffffff,
            0x00000000ffffffff_ffffffff00000000,
            0x00000000ffffffff_ffffffff00000000,
        ),
    ] {
        for (count, result) in [
            (0, input),
            (1, one),
            (width - 1, last),
            (width, oversized),
            (255, oversized),
            (1_u64 << 32, oversized),
            (1_u64 << 63, oversized),
        ] {
            // The high qword is ignored even when it would itself be an oversized count.
            let source = 0xfedcba9876543210_0000000000000000 | u128::from(count);
            check_binary(&mut checks, opcode, input, source, result);
            if count <= 255 {
                let code = [
                    0x66,
                    0x0f,
                    immediate,
                    0xc0 | extension << 3 | 7,
                    count as u8,
                ];
                let mut image = image(&code);
                image.cpu.simd.xmm[7] = input.to_le_bytes();
                let mut cpu = retire(image.cpu, code.len());
                cpu.simd.xmm[7] = result.to_le_bytes();
                checks.check(
                    "unsigned immediate shift count",
                    &code,
                    &image,
                    &[dispatch(cpu)],
                );
            }
        }
    }
    let code = [0x66, 0x0f, 0xf1, 0xff]; // PSLLW xmm7, xmm7
    let mut image = image(&code);
    image.cpu.simd.xmm[7] = 0x8001ffff7fff0002_0000000000000001_u128.to_le_bytes();
    let mut cpu = retire(image.cpu, code.len());
    cpu.simd.xmm[7] = 0x0002fffefffe0004_0000000000000002_u128.to_le_bytes();
    checks.check(
        "destination is also the count",
        &code,
        &image,
        &[dispatch(cpu)],
    );
}

fn count_span(engine: Engine, frontend: Frontend) {
    use wasm86_x86::{SegmentAttributes, SegmentDefaultSize, SegmentKind};
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Segmented16);
    let code = [0x66, 0x0f, 0xf1, 0x36, 0x00, 0x40]; // PSLLW xmm6, [disp16]
    let mut image = image(&code);
    image.cpu.segments.cs.attributes = SegmentAttributes::new(
        SegmentKind::Code { readable: true },
        SegmentDefaultSize::Bits16,
    );
    image.cpu.segments.ds.limit = 0x4007;
    image.cpu.simd.xmm[6] = [0; 16];
    image.map(4, 0x8000, false);
    image.data(0x8000, &[0; 16]);
    checks.check(
        "unused high count bits still belong to the m128 operand span",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::GeneralProtection { error: 0 },
        }],
    );
}

#[test]
fn sse_immediate_shifts_reject_memory_destinations_before_reading_more_bytes() {
    use crate::support::step::TestModule;
    use wasm86_x86::{compile_block_from_bytes, BlockError};
    for code in [
        [0x66, 0x0f, 0x71, 0x34],
        [0x66, 0x0f, 0x72, 0x24],
        [0x66, 0x0f, 0x73, 0x14],
    ] {
        assert_eq!(
            compile_block_from_bytes(0x1ffc, &code, 1).err(),
            Some(BlockError::UnsupportedInstruction {
                address: 0x1ffc,
                opcode: 0x0f
            }),
        );
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x1ffc;
        image.data(0x3ffc, &code);
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "invalid memory destination is rejected before missing SIB or immediate bytes",
            Exit::Other(0x0008_000f_0000_1ffc),
        );
    }
}

test_frontends!(sse_packed_shifts, shifts);
test_frontends!(sse_shift_count_span, count_span);
