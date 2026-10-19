//! Packed width conversion preserves lane effects and guards range boundaries.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use relocade_x86::{Compiler, ExecutionProfile, Segments};

fn lane_effects(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (narrow, sources, results, controls, raised) in [
        (
            false,
            [0x3f80_0000, 0xbf80_0000],
            [0x3ff0_0000_0000_0000, 0xbff0_0000_0000_0000],
            0x1f80,
            0,
        ),
        (
            true,
            [0x3ff0_0000_0000_0000, 0xbff0_0000_0000_0000],
            [0x3f80_0000, 0xbf80_0000],
            0x1f80,
            0,
        ),
        (
            true,
            [0x7ff0_0000_0000_0001, 0x47f0_0000_0000_0000],
            [0, 0],
            0x1f00,
            1,
        ),
        (true, [1, 0x47f0_0000_0000_0000], [0, 0], 0x1e80, 2),
        (
            true,
            [0x36a0_0000_0000_0000, 0x3ff0_0000_1000_0000],
            [0, 0],
            0x1780,
            48,
        ),
        (
            true,
            [0x47f0_0000_0000_0000, 0x3ff0_0000_1000_0000],
            [0, 0],
            0x1b80,
            40,
        ),
    ] {
        let code = encoding(prefix(narrow, true), 0x5a, 0xc0);
        let mut image = floating_image(&code, controls);
        for (index, bits) in sources.into_iter().enumerate() {
            lane(&mut image.cpu.simd.xmm[0], narrow, index, bits);
        }
        let fault = raised & !(controls >> 7) != 0;
        let mut cpu = if fault {
            image.cpu
        } else {
            retire(image.cpu, code.len())
        };
        cpu.simd.mxcsr |= raised;
        if !fault {
            cpu.simd.xmm[0] = [0; 16];
            for (index, bits) in results.into_iter().enumerate() {
                lane(&mut cpu.simd.xmm[0], !narrow, index, bits);
            }
        }
        checks.check(
            "packed widths collect all lane conditions before publication",
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

fn native_narrow() {
    // Both signs, ties, exact values and directed neighbors at the range edges.
    for (source, results, raised) in [
        (
            0x3ff0_0000_1000_0000_u64,
            [0x3f80_0000, 0x3f80_0000, 0x3f80_0001, 0x3f80_0000],
            32,
        ),
        (0x3ff8_0000_0000_0000, [0x3fc0_0000; 4], 0),
        (
            0x3810_0000_1800_0000,
            [0x0080_0001, 0x0080_0000, 0x0080_0001, 0x0080_0000],
            32,
        ),
        (
            0x47ef_ffff_c800_0000,
            [0x7f7f_fffe, 0x7f7f_fffe, 0x7f7f_ffff, 0x7f7f_fffe],
            32,
        ),
        (0, [0; 4], 0),
    ] {
        for rc in 0..4 {
            for controls in [0x1f80, 0x0f80, 0x1380, 0x9fc0] {
                let mut image = floating_image(&[0x66, 0x0f, 0x5a, 0xc1], controls | rc << 13);
                lane(&mut image.cpu.simd.xmm[1], true, 0, source);
                lane(&mut image.cpu.simd.xmm[1], true, 1, source | 1 << 63);
                let negative_rc = match rc {
                    1 => 2,
                    2 => 1,
                    _ => rc,
                };
                let result = [
                    results[rc as usize],
                    results[negative_rc as usize] | 0x8000_0000,
                ];
                check_native(image, result, raised, false);
            }
        }
    }
    let mut image = floating_image(&[0x66, 0x0f, 0x5a, 0xc1], 0x1fc0);
    lane(&mut image.cpu.simd.xmm[1], true, 0, 1);
    lane(&mut image.cpu.simd.xmm[1], true, 1, 0x8000_0000_0000_0001);
    check_native(image, [0, 0x8000_0000], 0, false);
}

fn restart_narrow() {
    for (source, result, raised) in [
        (0x3810_0000_0000_0000, 0x0080_0000, 0),
        (0x47ef_ffff_e000_0000, 0x7f7f_ffff, 0),
        (0x380f_ffff_e000_0000, 0x0080_0000, 48),
        (0x47ef_ffff_f000_0000, 0x7f80_0000, 40),
        (0xfff0_008a_c000_0123, 0xffc0_0456, 1),
    ] {
        let mut image = floating_image(&[0x90, 0x66, 0x0f, 0x5a, 0xc1], 0x1f80);
        lane(&mut image.cpu.simd.xmm[1], true, 0, source);
        lane(&mut image.cpu.simd.xmm[1], true, 1, ONE[1]);
        check_native(image, [result, ONE[0] as u32], raised, true);
    }
}

fn check_native(mut image: Image, result: [u32; 2], raised: u32, restart: bool) {
    let code: &[u8] = if restart {
        &[0x90, 0x66, 0x0f, 0x5a, 0xc1]
    } else {
        &[0x66, 0x0f, 0x5a, 0xc1]
    };
    let initial_cpu = image.cpu;
    for profile in [ExecutionProfile::Real16, SegmentProfile::Flat32.into()] {
        image.cpu = initial_cpu;
        if profile == ExecutionProfile::Real16 {
            image.cpu.segments = Segments::real_mode();
        }
        let prior = if restart {
            retire(image.cpu, 1)
        } else {
            image.cpu
        };
        let fault = raised & !(image.cpu.simd.mxcsr >> 7) != 0;
        let mut cpu = if fault { prior } else { retire(prior, 4) };
        cpu.simd.mxcsr |= raised;
        if !fault {
            cpu.simd.xmm[0] = [0; 16];
            for (index, bits) in result.into_iter().enumerate() {
                lane(&mut cpu.simd.xmm[0], false, index, u64::from(bits));
            }
        }
        let wanted = expected(
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
        for observed in [false, true] {
            let compiler = Compiler::new(profile);
            let compiler = if observed {
                compiler.specialize_on_cpu(&image.cpu)
            } else {
                compiler
            };
            let compiled = compiler
                .compile_block(0x1000, code, if restart { 2 } else { 1 })
                .unwrap();
            let block = TestModule::new(&compiled);
            if restart {
                assert_eq!(
                    block.observe(&image.input(), 1),
                    expected(
                        &image,
                        &[Step {
                            cpu: prior,
                            ram: &[],
                            exit: Exit::Interpret
                        }]
                    )
                );
                let linked = block.with_interpreter(TestModule::interpreter_with_profile(profile));
                assert_eq!(linked.observe(&image.input(), 1), wanted);
            } else {
                assert_eq!(
                    block.observe(&image.input(), 1),
                    wanted,
                    "source={:x?}, MXCSR={:x}, observed={observed}",
                    image.cpu.simd.xmm[1],
                    image.cpu.simd.mxcsr
                );
            }
        }
    }
}

fn widen_encodings(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let code = [0x0f, 0x5a, 0xc1];
    for (sources, results, controls, raised) in [
        (
            [0xffc0_0123, 0x7f80_0456],
            [0xfff8_0024_6000_0000, 0x7ff8_008a_c000_0000],
            0x1f80,
            1,
        ),
        (
            [0x8000_0001, 0x7f80_0456],
            [0xb6a0_0000_0000_0000, 0x7ff8_008a_c000_0000],
            0x1f80,
            3,
        ),
        (
            [0x3fc0_0000, 0x8000_0000],
            [0x3ff8_0000_0000_0000, 0x8000_0000_0000_0000],
            0x1f80,
            0,
        ),
        ([0x8000_0001, 1], [0x8000_0000_0000_0000, 0], 0x1fc0, 0),
    ] {
        let mut image = floating_image(&code, controls);
        for (index, bits) in sources.into_iter().enumerate() {
            lane(&mut image.cpu.simd.xmm[1], false, index, bits);
        }
        lane(&mut image.cpu.simd.xmm[1], false, 2, SNAN[0]);
        lane(&mut image.cpu.simd.xmm[1], false, 3, DENORMAL[0]);
        let mut cpu = retire(image.cpu, code.len());
        cpu.simd.mxcsr |= raised;
        for (index, bits) in results.into_iter().enumerate() {
            lane(&mut cpu.simd.xmm[0], true, index, bits);
        }
        checks.check(
            "packed widening keeps NaN encodings and ignores the upper source lanes",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
}

test_frontends!(sse_float_width_packed, lane_effects);
test_frontends!(sse_float_widen_encodings, widen_encodings);

fn native_widen() {
    let code = [0x90, 0x0f, 0x5a, 0xc1];
    for (source, controls, result, raised, restart) in [
        (0x3fc0_0000, 0x1f80, 0x3ff8_0000_0000_0000, 0, false),
        (0x8000_0001, 0x1fc0, 0x8000_0000_0000_0000, 0, false),
        (0xffc0_0123, 0x1f80, 0xfff8_0024_6000_0000, 0, true),
        (0x8000_0001, 0x1f80, 0xb6a0_0000_0000_0000, 2, true),
    ] {
        let mut image = floating_image(&code, controls);
        lane(&mut image.cpu.simd.xmm[1], false, 0, source);
        lane(&mut image.cpu.simd.xmm[1], false, 1, source);
        let prior = retire(image.cpu, 1);
        let mut cpu = retire(prior, 3);
        cpu.simd.mxcsr |= raised;
        for index in 0..2 {
            lane(&mut cpu.simd.xmm[0], true, index, result);
        }
        let wanted = expected(&image, &[dispatch(cpu)]);
        for observed in [false, true] {
            let compiler = Compiler::new(SegmentProfile::Flat32);
            let compiler = if observed {
                compiler.specialize_on_cpu(&image.cpu)
            } else {
                compiler
            };
            let compiled = compiler.compile_block(0x1000, &code, 2).unwrap();
            let block = TestModule::new(&compiled);
            if restart {
                assert_eq!(
                    block.observe(&image.input(), 1),
                    expected(
                        &image,
                        &[Step {
                            cpu: prior,
                            ram: &[],
                            exit: Exit::Interpret
                        }]
                    )
                );
                let linked = block.with_interpreter(TestModule::interpreter());
                assert_eq!(linked.observe(&image.input(), 1), wanted);
            } else {
                assert_eq!(block.observe(&image.input(), 1), wanted);
            }
        }
    }
}

#[test]
fn packed_width_native_and_restart() {
    native_narrow();
    restart_narrow();
    native_widen();
}
