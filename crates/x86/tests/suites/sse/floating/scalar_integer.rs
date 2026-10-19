//! Scalar conversion checks rounding before range, without changing XMM or flags.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use wasm86_x86::{Compiler, ExecutionProfile, Segments};

fn conversions(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        let mut cases = vec![
            (ZERO, [0; 4], [0; 4]),
            (NEG_ZERO, [0; 4], [0; 4]),
            (ONE, [1; 4], [0; 4]),
            (NEG_ONE, [-1; 4], [0; 4]),
            ([0x3fc0_0000, 0x3ff8_0000_0000_0000], [2, 1, 2, 1], [32; 4]),
            (
                [0xbfc0_0000, 0xbff8_0000_0000_0000],
                [-2, -2, -1, -1],
                [32; 4],
            ),
            ([0x4020_0000, 0x4004_0000_0000_0000], [2, 2, 3, 2], [32; 4]),
            (
                [0xc020_0000, 0xc004_0000_0000_0000],
                [-2, -3, -2, -2],
                [32; 4],
            ),
            (DENORMAL, [0, 0, 1, 0], [32; 4]),
            (NEG_DENORMAL, [0, -1, 0, 0], [32; 4]),
            (INFINITY, [i32::MIN; 4], [1; 4]),
            (NEG_INFINITY, [i32::MIN; 4], [1; 4]),
            (QNAN, [i32::MIN; 4], [1; 4]),
            (SNAN, [i32::MIN; 4], [1; 4]),
            ([0xcf00_0000, 0xc1e0_0000_0000_0000], [i32::MIN; 4], [0; 4]),
            ([0x4f00_0000, 0x41e0_0000_0000_0000], [i32::MIN; 4], [1; 4]),
        ];
        if wide {
            cases.extend([
                ([0, 2147483647.0_f64.to_bits()], [i32::MAX; 4], [0; 4]),
                (
                    [0, 2147483647.5_f64.to_bits()],
                    [i32::MIN, i32::MAX, i32::MIN, i32::MAX],
                    [1, 32, 1, 32],
                ),
                (
                    [0, (-2147483648.5_f64).to_bits()],
                    [i32::MIN; 4],
                    [32, 1, 32, 32],
                ),
                ([0, (-2147483649.0_f64).to_bits()], [i32::MIN; 4], [1; 4]),
            ]);
        } else {
            cases.push(([0x4eff_ffff, 0], [2147483520; 4], [0; 4]));
            cases.push(([0xcf00_0001, 0], [i32::MIN; 4], [1; 4]));
        }
        for truncate in [false, true] {
            let code = encoding(
                prefix(wide, false),
                if truncate { 0x2c } else { 0x2d },
                0xc1,
            );
            for &(bits, results, raised) in &cases {
                for rc in 0..4 {
                    for masks in [0x1f80, 0x1f00, 0x0f80, 0x1ec0] {
                        let daz = masks & 0x40 != 0 && (bits == DENORMAL || bits == NEG_DENORMAL);
                        let mode = if truncate { 3 } else { rc };
                        let status = if daz { 0 } else { raised[mode] };
                        let value = if daz { 0 } else { results[mode] };
                        let mxcsr = masks | (rc as u32) << 13;
                        let fault = status & !(mxcsr >> 7) != 0;
                        let mut image = floating_image(&code, mxcsr);
                        image.cpu.registers.eax = 0xa5a5_1234;
                        lane(&mut image.cpu.simd.xmm[1], wide, 0, bits[wide as usize]);
                        let mut cpu = if fault {
                            image.cpu
                        } else {
                            retire(image.cpu, code.len())
                        };
                        cpu.simd.mxcsr |= status;
                        if !fault {
                            cpu.registers.eax = value as u32;
                        }
                        let name = format!(
                            "CVT scalar wide={wide} trunc={truncate} bits={bits:x?} MXCSR={mxcsr:x}"
                        );
                        checks.check(
                            &name,
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

fn memory(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        let code = encoding(prefix(wide, false), 0x2c, 0x08); // ECX,[EAX]
        let width = if wide { 8 } else { 4 };
        for address in [0x4001, 0x5000 - width, 0x5001 - width] {
            let mut image = floating_image(&code, 0x1f80);
            image.cpu.registers.eax = address;
            image.map(4, 0x8000, false);
            let fault = address + width > 0x5000;
            image.data(
                address + 0x4000,
                &ONE[wide as usize].to_le_bytes()[..width as usize],
            );
            let mut cpu = image.cpu;
            if !fault {
                cpu = retire(cpu, code.len());
                cpu.registers.ecx = 1;
            }
            checks.check(
                "scalar conversion reads exactly one unaligned source",
                &code,
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: if fault {
                        Exit::PageFault {
                            address: 0x5000,
                            error: 0,
                        }
                    } else {
                        Exit::Dispatch(cpu.eip)
                    },
                }],
            );
        }
    }
}

#[test]
fn sse_scalar_integer_precision_stays_compiled() {
    for profile in [SegmentProfile::Flat32.into(), ExecutionProfile::Real16] {
        let code = [0xf3, 0x0f, 0x2d, 0xc1]; // CVTSS2SI EAX,XMM1
        let module = TestModule::new(
            &Compiler::new(profile)
                .compile_block(0x1000, &code, 1)
                .unwrap(),
        );
        for mxcsr in [0x1f80, 0x0f80] {
            let mut image = floating_image(&code, mxcsr);
            if profile == ExecutionProfile::Real16 {
                image.cpu.segments = Segments::real_mode();
            }
            lane(&mut image.cpu.simd.xmm[1], false, 0, 0x3fc0_0000);
            let fault = mxcsr == 0x0f80;
            let mut cpu = if fault {
                image.cpu
            } else {
                retire(image.cpu, code.len())
            };
            cpu.simd.mxcsr |= 32;
            if !fault {
                cpu.registers.eax = 2;
            }
            assert_eq!(
                module.observe(&image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu,
                        ram: &[],
                        exit: if fault {
                            Exit::SimdFloatingPoint
                        } else {
                            Exit::Dispatch(cpu.eip)
                        }
                    }]
                )
            );
        }
    }
}

test_frontends!(sse_scalar_integer_conversion, conversions);
test_frontends!(sse_scalar_integer_memory, memory);
