//! Native scalar and packed arithmetic keep precise flags and an explicit restart domain.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use wasm86_x86::{Compiler, ExecutionProfile, Segments};

struct NativeCase {
    opcode: u8,
    left: [u64; 2],
    right: [u64; 2],
    result: [u64; 2],
    raised: u32,
}

fn check(cases: &[NativeCase], restart: bool) {
    for profile in [ExecutionProfile::Real16, SegmentProfile::Flat32.into()] {
        for wide in [false, true] {
            for packed in [false, true] {
                for case in cases {
                    let instruction = encoding(prefix(wide, packed), case.opcode, 0xc1);
                    let code = [&[0x90][..], &instruction].concat();
                    for controls in [0x1f80, 0x1fa0, 0x0f80, 0x0fa0, 0x9fc0] {
                        let mut image = floating_image(&code, controls);
                        if profile == ExecutionProfile::Real16 {
                            image.cpu.segments = Segments::real_mode();
                        }
                        let lanes = if wide { 2 } else { 4 };
                        let count = if packed { lanes } else { 1 };
                        // Unused scalar lanes must neither cause a restart nor change flags.
                        for index in count..lanes {
                            lane(&mut image.cpu.simd.xmm[0], wide, index, SNAN[wide as usize]);
                            lane(&mut image.cpu.simd.xmm[1], wide, index, 1);
                        }
                        // Exact packed siblings ensure the final lane alone determines
                        // precision, including the highest dword/qword mask bit.
                        for index in 0..count {
                            lane(&mut image.cpu.simd.xmm[0], wide, index, ONE[wide as usize]);
                            lane(&mut image.cpu.simd.xmm[1], wide, index, ONE[wide as usize]);
                        }
                        lane(
                            &mut image.cpu.simd.xmm[0],
                            wide,
                            count - 1,
                            case.left[wide as usize],
                        );
                        lane(
                            &mut image.cpu.simd.xmm[1],
                            wide,
                            count - 1,
                            case.right[wide as usize],
                        );
                        let prior = retire(image.cpu, 1);
                        let fault = case.raised & !(controls >> 7) != 0;
                        let mut cpu = if fault {
                            prior
                        } else {
                            retire(prior, instruction.len())
                        };
                        cpu.simd.mxcsr |= case.raised;
                        if !fault {
                            let neutral = match case.opcode {
                                0x58 => TWO,
                                0x5c => ZERO,
                                _ => ONE,
                            };
                            for index in 0..count {
                                lane(&mut cpu.simd.xmm[0], wide, index, neutral[wide as usize]);
                            }
                            lane(
                                &mut cpu.simd.xmm[0],
                                wide,
                                count - 1,
                                case.result[wide as usize],
                            );
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
                                let linked = block.with_interpreter(
                                    TestModule::interpreter_with_profile(profile),
                                );
                                assert_eq!(linked.observe(&image.input(), 1), wanted);
                            } else {
                                assert_eq!(
                                    block.observe( &image.input(), 1),
                                    wanted,
                                    "native {:x}, wide={wide}, packed={packed}, MXCSR={controls:x}, observed={observed}",
                                    case.opcode
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

fn precision() {
    check(
        &[
            NativeCase {
                opcode: 0x58,
                left: ONE,
                right: [0x0d80_0000, 0x0170_0000_0000_0000],
                result: ONE,
                raised: 32,
            }, // tiny normal addend
            NativeCase {
                opcode: 0x58,
                left: MIN_NORMAL,
                right: [0x0080_0001, 0x0010_0000_0000_0001],
                result: [0x0100_0000, 0x0020_0000_0000_0000],
                raised: 32,
            }, // subnormal error
            NativeCase {
                opcode: 0x58,
                left: ONE,
                right: TWO,
                result: [0x4040_0000, 0x4008_0000_0000_0000],
                raised: 0,
            },
            NativeCase {
                opcode: 0x5c,
                left: ONE,
                right: ONE,
                result: ZERO,
                raised: 0,
            },
            NativeCase {
                opcode: 0x5c,
                left: NEG_ZERO,
                right: ZERO,
                result: NEG_ZERO,
                raised: 0,
            },
            NativeCase {
                opcode: 0x59,
                left: NEXT_ONE,
                right: [0x3fff_fffe, 0x3fff_ffff_ffff_fffe],
                result: TWO,
                raised: 32,
            }, // rounding crosses a binade
            NativeCase {
                opcode: 0x59,
                left: [0x3fff_fffe, 0x3fff_ffff_ffff_fffe],
                right: [0x3fc0_0000, 0x3ff8_0000_0000_0000],
                result: [0x403f_fffe, 0x4007_ffff_ffff_fffe],
                raised: 32,
            }, // the higher binade discards one additional significand bit
            NativeCase {
                opcode: 0x59,
                left: [0x3f80_0002, 0x3ff0_0000_0000_0002],
                right: [0x3fc0_0000, 0x3ff8_0000_0000_0000],
                result: [0x3fc0_0003, 0x3ff8_0000_0000_0003],
                raised: 0,
            }, // the lower binade retains that bit
            NativeCase {
                opcode: 0x59,
                left: [0x3fc0_0000, 0x3ff8_0000_0000_0000],
                right: TWO,
                result: [0x4040_0000, 0x4008_0000_0000_0000],
                raised: 0,
            },
            NativeCase {
                opcode: 0x5e,
                left: ONE,
                right: [0x4040_0000, 0x4008_0000_0000_0000],
                result: [0x3eaa_aaab, 0x3fd5_5555_5555_5555],
                raised: 32,
            },
            NativeCase {
                opcode: 0x5e,
                left: MIN_NORMAL,
                right: [0x3f40_0000, 0x3fe8_0000_0000_0000],
                result: [0x00aa_aaab, 0x0015_5555_5555_5555],
                raised: 32,
            },
            NativeCase {
                opcode: 0x5e,
                left: TWO,
                right: TWO,
                result: ONE,
                raised: 0,
            },
            NativeCase {
                opcode: 0x51,
                left: ZERO,
                right: TWO,
                result: [0x3fb5_04f3, 0x3ff6_a09e_667f_3bcd],
                raised: 32,
            },
            NativeCase {
                opcode: 0x51,
                left: ZERO,
                right: [0x4080_0000, 0x4010_0000_0000_0000],
                result: TWO,
                raised: 0,
            },
            NativeCase {
                opcode: 0x51,
                left: ZERO,
                right: NEG_ZERO,
                result: NEG_ZERO,
                raised: 0,
            },
        ],
        false,
    );
}

fn restart() {
    // All controls used by check retain these results; DAZ does not participate.
    check(
        &[
            NativeCase {
                opcode: 0x58,
                left: ONE,
                right: SNAN,
                result: QUIETED,
                raised: 1,
            },
            NativeCase {
                opcode: 0x59,
                left: MIN_NORMAL,
                right: ONE,
                result: MIN_NORMAL,
                raised: 0,
            },
            NativeCase {
                opcode: 0x59,
                left: MAX_FINITE,
                right: ONE,
                result: MAX_FINITE,
                raised: 0,
            },
            NativeCase {
                opcode: 0x59,
                left: MIN_NORMAL,
                right: MIN_NORMAL,
                result: ZERO,
                raised: 48,
            },
            NativeCase {
                opcode: 0x5e,
                left: ONE,
                right: ZERO,
                result: INFINITY,
                raised: 4,
            },
            NativeCase {
                opcode: 0x51,
                left: ZERO,
                right: NEG_TWO,
                result: INDEFINITE,
                raised: 1,
            },
        ],
        true,
    );
}

#[test]
fn native_precision_and_restart() {
    precision();
    restart();
}
