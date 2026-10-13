//! MIN/MAX preserve the source's exact NaN encoding and signed zero on ties.

use super::*;

fn selection(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let cases = [
        (ONE, TWO, ONE, TWO, false),
        (NEG_ONE, NEG_TWO, NEG_TWO, NEG_ONE, false),
        (NEG_ZERO, ZERO, ZERO, ZERO, false),
        (ZERO, NEG_ZERO, NEG_ZERO, NEG_ZERO, false),
        (INFINITY, NEG_INFINITY, NEG_INFINITY, INFINITY, false),
        (QNAN, ONE, ONE, ONE, true),
        (ONE, QNAN, QNAN, QNAN, true),
        (SNAN, ONE, ONE, ONE, true),
        (ONE, SNAN, SNAN, SNAN, true),
        (QNAN, SNAN, SNAN, SNAN, true),
    ];
    for wide in [false, true] {
        for packed in [false, true] {
            for maximum in [false, true] {
                let code = encoding(
                    prefix(wide, packed),
                    if maximum { 0x5f } else { 0x5d },
                    0xc1,
                );
                let count = if packed {
                    16 / if wide { 8 } else { 4 }
                } else {
                    1
                };
                for start in 0..cases.len() {
                    for mxcsr in [0x1f80, 0x1f00] {
                        let mut image = floating_image(&code, mxcsr);
                        let mut expected = image.cpu.simd.xmm[0];
                        let mut invalid = false;
                        for index in 0..count {
                            let (left, right, min, max, nan) = cases[(start + index) % cases.len()];
                            lane(&mut image.cpu.simd.xmm[0], wide, index, left[wide as usize]);
                            lane(
                                &mut image.cpu.simd.xmm[1],
                                wide,
                                index,
                                right[wide as usize],
                            );
                            lane(
                                &mut expected,
                                wide,
                                index,
                                if maximum { max } else { min }[wide as usize],
                            );
                            invalid |= nan;
                        }
                        let fault = invalid && mxcsr == 0x1f00;
                        let mut cpu = if fault {
                            image.cpu
                        } else {
                            retire(image.cpu, code.len())
                        };
                        cpu.simd.mxcsr |= u32::from(invalid);
                        if !fault {
                            cpu.simd.xmm[0] = expected;
                        }
                        checks.check(
                            "MIN/MAX source selection, NaN bits, and whole-destination faults",
                            &code,
                            &image,
                            &[Step {
                                cpu,
                                ram: &[],
                                exit: if fault {
                                    Exit::SimdFloatingPoint
                                } else {
                                    Exit::Dispatch(cpu.eip)
                                },
                            }],
                        );
                    }
                }
            }
        }
    }
}

fn denormals(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // DAZ makes these input pairs equal zeros; the right operand wins either operation.
    for (left, right, min, max, daz_result) in [
        (NEG_DENORMAL, ZERO, NEG_DENORMAL, ZERO, ZERO),
        (ZERO, NEG_DENORMAL, NEG_DENORMAL, ZERO, NEG_ZERO),
        (DENORMAL, NEG_DENORMAL, NEG_DENORMAL, DENORMAL, NEG_ZERO),
    ] {
        for wide in [false, true] {
            for maximum in [false, true] {
                let code = encoding(prefix(wide, false), if maximum { 0x5f } else { 0x5d }, 0xc1);
                for (mxcsr, raised, fault) in [
                    (0x1f80, 2, false),
                    (0x9f80, 2, false),
                    (0x1e80, 2, true),
                    (0x1fc0, 0, false),
                ] {
                    let mut image = floating_image(&code, mxcsr);
                    lane(&mut image.cpu.simd.xmm[0], wide, 0, left[wide as usize]);
                    lane(&mut image.cpu.simd.xmm[1], wide, 0, right[wide as usize]);
                    let mut cpu = if fault {
                        image.cpu
                    } else {
                        retire(image.cpu, code.len())
                    };
                    cpu.simd.mxcsr |= raised;
                    if !fault {
                        let bits = if mxcsr & 0x40 != 0 {
                            daz_result
                        } else if maximum {
                            max
                        } else {
                            min
                        };
                        lane(&mut cpu.simd.xmm[0], wide, 0, bits[wide as usize]);
                    }
                    checks.check(
                        "MIN/MAX DAZ preserves zero sign; FTZ does not flush selected subnormals",
                        &code,
                        &image,
                        &[Step {
                            cpu,
                            ram: &[],
                            exit: if fault {
                                Exit::SimdFloatingPoint
                            } else {
                                Exit::Dispatch(cpu.eip)
                            },
                        }],
                    );
                }
            }
        }
    }
}

fn nan_denormal(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for maximum in [false, true] {
        let code = encoding(&[0xf3], if maximum { 0x5f } else { 0x5d }, 0xc1);
        for (mxcsr, bits) in [(0x1f80, NEG_DENORMAL[0]), (0x1fc0, NEG_ZERO[0])] {
            let mut image = floating_image(&code, mxcsr);
            lane(&mut image.cpu.simd.xmm[0], false, 0, QNAN[0]);
            lane(&mut image.cpu.simd.xmm[1], false, 0, NEG_DENORMAL[0]);
            let mut cpu = retire(image.cpu, code.len());
            cpu.simd.mxcsr |= 1;
            lane(&mut cpu.simd.xmm[0], false, 0, bits);
            checks.check(
                "NaN suppresses denormal status while DAZ still normalizes the source",
                &code,
                &image,
                &[dispatch(cpu)],
            );
        }
    }
}

fn memory(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        for packed in [false, true] {
            for maximum in [false, true] {
                let code = encoding(
                    prefix(wide, packed),
                    if maximum { 0x5f } else { 0x5d },
                    0x00,
                );
                let width = if packed {
                    16
                } else if wide {
                    8
                } else {
                    4
                };
                let count = width / if wide { 8 } else { 4 };
                let mut image = floating_image(&code, 0x1f80);
                let mut source = [0; 16];
                for index in 0..count {
                    lane(&mut image.cpu.simd.xmm[0], wide, index, TWO[wide as usize]);
                    lane(&mut source, wide, index, ONE[wide as usize]);
                }
                image.cpu.registers.eax = 0x5000 - width as u32;
                image.map(4, 0x8000, false);
                image.data(0x9000 - width as u32, &source[..width]);
                let mut cpu = retire(image.cpu, code.len());
                if !maximum {
                    cpu.simd.xmm[0][..width].copy_from_slice(&source[..width]);
                }
                checks.check(
                    "MIN/MAX memory forms access exactly their declared width",
                    &code,
                    &image,
                    &[dispatch(cpu)],
                );
            }
        }
    }
}

test_frontends!(sse_float_minmax_selection, selection);
test_frontends!(sse_float_minmax_denormals, denormals);
test_frontends!(sse_float_minmax_nan_denormal, nan_denormal);
test_frontends!(sse_float_minmax_memory, memory);
