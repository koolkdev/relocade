//! Floating layout instructions move raw encodings without FP exceptions.

use super::*;

const SINGLE_LEFT: u128 = 0xff800000_00000001_80000000_7f800123;
const SINGLE_RIGHT: u128 = 0x7fc12345_80000001_00000000_ff800456;
const DOUBLE_LEFT: u128 = 0x8000000000000000_7ff0000000000123;
const DOUBLE_RIGHT: u128 = 0x0000000000000001_fff8000000000456;

fn check_layout(
    checks: &mut ImageSequences,
    prefix: &[u8],
    opcode: u8,
    control: Option<u8>,
    left: u128,
    right: u128,
    result: u128,
) {
    for modrm in [0xf2, 0x30] {
        let mut code = encoding(prefix, opcode, modrm);
        code.extend(control);
        let mut image = image(&code);
        image.cpu.simd.xmm[6] = left.to_le_bytes();
        image.cpu.simd.xmm[2] = right.to_le_bytes();
        image.cpu.simd.mxcsr = 0x0045; // unmasked exceptions with preexisting status flags
        image.cpu.x87.status.error_summary = 1;
        image.cpu.registers.eax = 0x4ff0;
        image.map(4, 0x8000, false);
        image.data(0x8ff0, &right.to_le_bytes());
        let mut cpu = retire(image.cpu, code.len());
        cpu.simd.xmm[6] = result.to_le_bytes();
        checks.check(
            "layout preserves encodings and does not consult or change FP status",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
}

fn shuffle(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (prefix, width, left, right) in [
        (&[][..], 4, SINGLE_LEFT, SINGLE_RIGHT),
        (&[0x66][..], 8, DOUBLE_LEFT, DOUBLE_RIGHT),
    ] {
        for control in [0, 1, 2, 3, 0x1b, 0xe4, 0xfc, 0xfd, 0xfe, 0xff] {
            let mut result = [0; 16];
            for lane in 0..16 / width {
                let (input, selector) = if width == 4 {
                    (
                        if lane < 2 { left } else { right },
                        (control >> (lane * 2)) & 3,
                    )
                } else {
                    (if lane == 0 { left } else { right }, (control >> lane) & 1)
                };
                let offset = usize::from(selector) * width;
                result[lane * width..(lane + 1) * width]
                    .copy_from_slice(&input.to_le_bytes()[offset..offset + width]);
            }
            check_layout(
                &mut checks,
                prefix,
                0xc6,
                Some(control),
                left,
                right,
                u128::from_le_bytes(result),
            );
        }
    }
    let code = [0x0f, 0xc6, 0xf6, 0x1b];
    let mut image = image(&code);
    image.cpu.simd.xmm[6] = SINGLE_LEFT.to_le_bytes();
    let mut cpu = retire(image.cpu, 4);
    cpu.simd.xmm[6] = 0x7f800123_80000000_00000001_ff800000_u128.to_le_bytes();
    checks.check(
        "self-shuffle snapshots every source lane",
        &code,
        &image,
        &[dispatch(cpu)],
    );
}

fn unpack(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (prefix, opcode, left, right, result) in [
        (
            &[][..],
            0x14,
            SINGLE_LEFT,
            SINGLE_RIGHT,
            0x00000000_80000000_ff800456_7f800123,
        ),
        (
            &[][..],
            0x15,
            SINGLE_LEFT,
            SINGLE_RIGHT,
            0x7fc12345_ff800000_80000001_00000001,
        ),
        (
            &[0x66][..],
            0x14,
            DOUBLE_LEFT,
            DOUBLE_RIGHT,
            0xfff8000000000456_7ff0000000000123,
        ),
        (
            &[0x66][..],
            0x15,
            DOUBLE_LEFT,
            DOUBLE_RIGHT,
            0x0000000000000001_8000000000000000,
        ),
    ] {
        check_layout(&mut checks, prefix, opcode, None, left, right, result);
    }
}

fn faults(frontend: Frontend) {
    let mut flat = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let mut segmented = ImageSequences::new(frontend, SegmentProfile::Segmented16);
    for prefix in [&[][..], &[0x66][..]] {
        for opcode in [0xc6, 0x14, 0x15] {
            let mut code = encoding(prefix, opcode, 0x30);
            if opcode == 0xc6 {
                code.push(0);
            }
            for (address, exit) in [
                (
                    0x5000,
                    Exit::PageFault {
                        address: 0x5000,
                        error: 0,
                    },
                ),
                (0x5008, Exit::GeneralProtection { error: 0 }),
            ] {
                let mut image = image(&code);
                image.cpu.registers.eax = address;
                flat.check(
                    "layout checks aligned memory before destination effects",
                    &code,
                    &image,
                    &[Step {
                        cpu: image.cpu,
                        ram: &[],
                        exit,
                    }],
                );
            }
            let mut code = encoding(prefix, opcode, 0x36);
            code.extend([0x00, 0x40]);
            if opcode == 0xc6 {
                code.push(0);
            }
            check_m128_span(&mut segmented, &code);
        }
    }
}

test_frontends!(sse_float_shuffle, shuffle);
test_frontends!(sse_float_unpack, unpack);
test_frontends!(sse_float_layout_faults, faults);
