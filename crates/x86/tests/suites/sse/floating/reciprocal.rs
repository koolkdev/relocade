//! Estimates obey their accuracy bound and leave every MXCSR bit unchanged.

use super::*;
use crate::support::{
    machine::expected,
    step::{Event, TestModule},
};
use relocade_x86::compile_block_from_bytes;

const ERROR: f64 = 1.5 / 4096.0;

fn assert_reciprocal(source: u32, result: u32) {
    let x = f64::from(f32::from_bits(source));
    let y = f64::from(f32::from_bits(result));
    assert_eq!(result >> 31, source >> 31, "reciprocal keeps its sign");
    assert!(y.is_finite(), "finite normal input has a finite reciprocal");
    assert!(y == 0.0 || y.abs() >= f64::from(f32::MIN_POSITIVE));
    if y == 0.0 {
        // The allowed estimate can cross the normal boundary. Zero is valid
        // only if some result within the specified error bound would be tiny.
        assert!(x.abs() * f64::from(f32::MIN_POSITIVE) > 1.0 - ERROR);
    } else {
        // A binary32-by-binary32 product is exact in binary64. This checks the
        // architectural error bound without reproducing the division algorithm.
        assert!((x * y - 1.0).abs() <= ERROR, "{source:08x} -> {result:08x}");
    }
}

fn assert_reciprocal_square_root(source: u32, result: u32) {
    if source >> 31 != 0 {
        assert_eq!(
            result, 0xffc0_0000,
            "negative normal input yields indefinite"
        );
        return;
    }
    let x = f64::from(f32::from_bits(source));
    let y = f64::from(f32::from_bits(result));
    assert!(y.is_finite() && y >= f64::from(f32::MIN_POSITIVE));
    // Squaring the relative-error interval avoids using sqrt or reciprocal
    // in the oracle. All positive normal inputs have normal finite estimates.
    assert!(
        ((1.0 - ERROR).powi(2)..=(1.0 + ERROR).powi(2)).contains(&(x * y * y)),
        "{source:08x} -> {result:08x}"
    );
}

fn accuracy(frontend: Frontend, opcode: u8, check: fn(u32, u32)) {
    for packed in [false, true] {
        let code = encoding(prefix(false, packed), opcode, 0xc1);
        let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
        let module = match frontend {
            Frontend::Block => &block,
            Frontend::Interpreter => TestModule::interpreter(),
        };
        let mut sources = Vec::new();
        for exponent in 1..255 {
            if !packed && ![1, 2, 63, 126, 127, 128, 191, 253, 254].contains(&exponent) {
                continue;
            }
            for fraction in [0, 1, 0x400000, 0x7fffff] {
                for sign in [0, 0x8000_0000] {
                    sources.push(sign | exponent << 23 | fraction);
                }
            }
        }
        for values in sources.chunks_exact(if packed { 4 } else { 1 }) {
            // Unmasked exceptions and existing sticky flags.
            let mut image = floating_image(&code, 0x3f);
            // Unused scalar lanes neither participate nor lose their raw encodings.
            lane(&mut image.cpu.simd.xmm[0], false, 1, SNAN[0]);
            lane(&mut image.cpu.simd.xmm[1], false, 1, SNAN[0]);
            for (index, &bits) in values.iter().enumerate() {
                lane(&mut image.cpu.simd.xmm[1], false, index, u64::from(bits));
            }
            let actual = module.observe(&image.input(), 1);
            let snapshot = actual
                .events
                .iter()
                .find_map(|event| match event {
                    Event::Dispatch { snapshot, .. } => Some(snapshot),
                    _ => None,
                })
                .expect("reciprocal stays compiled and dispatches without a numerical fault");
            let result_cpu = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
            let mut cpu = retire(image.cpu, code.len());
            for (index, &source) in values.iter().enumerate() {
                let result = u32::from_le_bytes(
                    result_cpu.simd.xmm[0][index * 4..index * 4 + 4]
                        .try_into()
                        .unwrap(),
                );
                check(source, result);
                lane(&mut cpu.simd.xmm[0], false, index, u64::from(result));
            }
            assert_eq!(actual, expected(&image, &[dispatch(cpu)]));
        }
    }
}

fn reciprocal_special(frontend: Frontend) {
    special(
        frontend,
        0x53,
        &[
            (0_u32, 0x7f80_0000_u32),
            (0x8000_0000, 0xff80_0000),
            (1, 0x7f80_0000),
            (0x8000_0001, 0xff80_0000),
            (0x007f_ffff, 0x7f80_0000),
            (0x807f_ffff, 0xff80_0000),
            (0x7f80_0000, 0),
            (0xff80_0000, 0x8000_0000),
            (0x7fc0_0123, 0x7fc0_0123),
            (0xffc0_0123, 0xffc0_0123),
            (0x7f80_0456, 0x7fc0_0456),
            (0xff80_0456, 0xffc0_0456),
            (0x7f7f_ffff, 0),
            (0xff7f_ffff, 0x8000_0000),
        ],
    );
}

fn reciprocal_square_root_special(frontend: Frontend) {
    special(
        frontend,
        0x52,
        &[
            (0, 0x7f80_0000),
            (0x8000_0000, 0xff80_0000),
            (1, 0x7f80_0000),
            (0x8000_0001, 0xff80_0000),
            (0x007f_ffff, 0x7f80_0000),
            (0x807f_ffff, 0xff80_0000),
            (0x7f80_0000, 0),
            (0xff80_0000, 0xffc0_0000),
            (0x8080_0000, 0xffc0_0000),
            (0xbf80_0000, 0xffc0_0000),
            (0xff7f_ffff, 0xffc0_0000),
            (0x7fc0_0123, 0x7fc0_0123),
            (0xffc0_0123, 0xffc0_0123),
            (0x7f80_0456, 0x7fc0_0456),
            (0xff80_0456, 0xffc0_0456),
        ],
    );
}

fn special(frontend: Frontend, opcode: u8, cases: &[(u32, u32)]) {
    for packed in [false, true] {
        let code = encoding(prefix(false, packed), opcode, 0xc1);
        let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
        let module = match frontend {
            Frontend::Block => &block,
            Frontend::Interpreter => TestModule::interpreter(),
        };
        for rc in 0..4 {
            for controls in [0, 0x1f80, 0x8040, 0x9fc0] {
                for start in 0..cases.len() {
                    let mut image = floating_image(&code, controls | rc << 13 | 0x25);
                    // Destination NaNs and unused source lanes have no effect.
                    lane(&mut image.cpu.simd.xmm[0], false, 0, SNAN[0]);
                    lane(&mut image.cpu.simd.xmm[0], false, 1, SNAN[0]);
                    lane(&mut image.cpu.simd.xmm[1], false, 1, SNAN[0]);
                    let mut result = image.cpu.simd.xmm[0];
                    for index in 0..if packed { 4 } else { 1 } {
                        let (source, bits) = cases[(start + index) % cases.len()];
                        lane(&mut image.cpu.simd.xmm[1], false, index, u64::from(source));
                        lane(&mut result, false, index, u64::from(bits));
                    }
                    let mut cpu = retire(image.cpu, code.len());
                    cpu.simd.xmm[0] = result;
                    assert_eq!(
                        module.observe(&image.input(), 1),
                        expected(&image, &[dispatch(cpu)])
                    );
                }
            }
        }
    }
    // Source/destination aliasing still preserves the unused scalar lanes.
    let code = encoding(&[0xf3], opcode, 0xc0);
    let mut image = floating_image(&code, 0);
    lane(&mut image.cpu.simd.xmm[0], false, 0, 0);
    let mut cpu = retire(image.cpu, code.len());
    lane(&mut cpu.simd.xmm[0], false, 0, INFINITY[0]);
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    let module = match frontend {
        Frontend::Block => &block,
        Frontend::Interpreter => TestModule::interpreter(),
    };
    assert_eq!(
        module.observe(&image.input(), 1),
        expected(&image, &[dispatch(cpu)])
    );
}

fn memory(frontend: Frontend, opcode: u8) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for packed in [false, true] {
        let code = encoding(prefix(false, packed), opcode, 0x00);
        let width = if packed { 16 } else { 4 };
        let mut image = floating_image(&code, 0);
        image.cpu.registers.eax = 0x5000 - width;
        image.map(4, 0x8000, false);
        image.data(0x9000 - width, &[0; 16][..width as usize]);
        let mut cpu = retire(image.cpu, code.len());
        for index in 0..if packed { 4 } else { 1 } {
            lane(&mut cpu.simd.xmm[0], false, index, INFINITY[0]);
        }
        checks.check(
            "reciprocal reads exactly m32 or aligned m128",
            &code,
            &image,
            &[dispatch(cpu)],
        );
        image.cpu.registers.eax = if packed { 0x4001 } else { 0x5000 };
        checks.check(
            "operand fault precedes destination update",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: if packed {
                    Exit::GeneralProtection { error: 0 }
                } else {
                    Exit::PageFault {
                        address: 0x5000,
                        error: 0,
                    }
                },
            }],
        );
    }
}

test_frontends!(@tests sse_reciprocal_accuracy, |frontend| accuracy(
    frontend,
    0x53,
    assert_reciprocal
));
test_frontends!(sse_reciprocal_special_and_controls, reciprocal_special);
test_frontends!(@tests sse_reciprocal_memory, |frontend| memory(
    frontend, 0x53
));
test_frontends!(@tests sse_reciprocal_square_root_accuracy, |frontend| {
    accuracy(frontend, 0x52, assert_reciprocal_square_root)
});
test_frontends!(
    sse_reciprocal_square_root_special_and_controls,
    reciprocal_square_root_special
);
test_frontends!(@tests sse_reciprocal_square_root_memory,
    |frontend| memory(frontend, 0x52)
);
