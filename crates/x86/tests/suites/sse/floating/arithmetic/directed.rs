//! Native directed rounding preserves exact direction, sticky status and scalar isolation.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use wasm86_x86::Compiler;

struct DirectionCase {
    opcode: u8,
    left: [u64; 2],
    right: [u64; 2],
    /// Round down, up and toward zero; each pair is binary32/binary64.
    results: [[u64; 2]; 3],
    raised: u32,
}

const CASES: &[DirectionCase] = &[
    // addition with subnormal residual
    DirectionCase {
        opcode: 0x58,
        left: [0x00800000, 0x10000000000000],
        right: [0x00800001, 0x10000000000001],
        results: [
            [0x01000000, 0x20000000000000],
            [0x01000001, 0x20000000000001],
            [0x01000000, 0x20000000000000],
        ],
        raised: 32,
    },
    // divide correction reaches minimum normal
    DirectionCase {
        opcode: 0x5e,
        left: [0x00800002, 0x10000000000002],
        right: [0x3f800001, 0x3ff0000000000001],
        results: [
            [0x00800000, 0x10000000000000],
            [0x00800001, 0x10000000000001],
            [0x00800000, 0x10000000000000],
        ],
        raised: 32,
    },
    // multiply correction reaches maximum finite
    DirectionCase {
        opcode: 0x59,
        left: [0x7f7fffff, 0x7fefffffffffffff],
        right: [0x3f7fffff, 0x3fefffffffffffff],
        results: [
            [0x7f7ffffe, 0x7feffffffffffffe],
            [0x7f7fffff, 0x7fefffffffffffff],
            [0x7f7ffffe, 0x7feffffffffffffe],
        ],
        raised: 32,
    },
    // add half ulp
    DirectionCase {
        opcode: 0x58,
        left: [0x3f800000, 0x3ff0000000000000],
        right: [0x33800000, 0x3ca0000000000000],
        results: [
            [0x3f800000, 0x3ff0000000000000],
            [0x3f800001, 0x3ff0000000000001],
            [0x3f800000, 0x3ff0000000000000],
        ],
        raised: 32,
    },
    // subtract halfway below one
    DirectionCase {
        opcode: 0x5c,
        left: [0x3f800000, 0x3ff0000000000000],
        right: [0x33000000, 0x3c90000000000000],
        results: [
            [0x3f7fffff, 0x3fefffffffffffff],
            [0x3f800000, 0x3ff0000000000000],
            [0x3f7fffff, 0x3fefffffffffffff],
        ],
        raised: 32,
    },
    // multiply next one
    DirectionCase {
        opcode: 0x59,
        left: [0x3f800001, 0x3ff0000000000001],
        right: [0x3f800001, 0x3ff0000000000001],
        results: [
            [0x3f800002, 0x3ff0000000000002],
            [0x3f800003, 0x3ff0000000000003],
            [0x3f800002, 0x3ff0000000000002],
        ],
        raised: 32,
    },
    // multiply carried binade
    DirectionCase {
        opcode: 0x59,
        left: [0x3fc00001, 0x3ff8000000000001],
        right: [0x3fc00001, 0x3ff8000000000001],
        results: [
            [0x40100001, 0x4002000000000001],
            [0x40100002, 0x4002000000000002],
            [0x40100001, 0x4002000000000001],
        ],
        raised: 32,
    },
    // multiply rounding across binade
    DirectionCase {
        opcode: 0x59,
        left: [0x3f800001, 0x3ff0000000000001],
        right: [0x3ffffffe, 0x3ffffffffffffffe],
        results: [
            [0x3fffffff, 0x3fffffffffffffff],
            [0x40000000, 0x4000000000000000],
            [0x3fffffff, 0x3fffffffffffffff],
        ],
        raised: 32,
    },
    // exact negative product
    DirectionCase {
        opcode: 0x59,
        left: [0xbfc00000, 0xbff8000000000000],
        right: [0x40000000, 0x4000000000000000],
        results: [
            [0xc0400000, 0xc008000000000000],
            [0xc0400000, 0xc008000000000000],
            [0xc0400000, 0xc008000000000000],
        ],
        raised: 0,
    },
    // divide one by three
    DirectionCase {
        opcode: 0x5e,
        left: [0x3f800000, 0x3ff0000000000000],
        right: [0x40400000, 0x4008000000000000],
        results: [
            [0x3eaaaaaa, 0x3fd5555555555555],
            [0x3eaaaaab, 0x3fd5555555555556],
            [0x3eaaaaaa, 0x3fd5555555555555],
        ],
        raised: 32,
    },
    // divide negative numerator
    DirectionCase {
        opcode: 0x5e,
        left: [0xbf800000, 0xbff0000000000000],
        right: [0x40400000, 0x4008000000000000],
        results: [
            [0xbeaaaaab, 0xbfd5555555555556],
            [0xbeaaaaaa, 0xbfd5555555555555],
            [0xbeaaaaaa, 0xbfd5555555555555],
        ],
        raised: 32,
    },
    // divide negative denominator
    DirectionCase {
        opcode: 0x5e,
        left: [0x3f800000, 0x3ff0000000000000],
        right: [0xc0400000, 0xc008000000000000],
        results: [
            [0xbeaaaaab, 0xbfd5555555555556],
            [0xbeaaaaaa, 0xbfd5555555555555],
            [0xbeaaaaaa, 0xbfd5555555555555],
        ],
        raised: 32,
    },
    // exact negative quotient
    DirectionCase {
        opcode: 0x5e,
        left: [0xc0400000, 0xc008000000000000],
        right: [0x40000000, 0x4000000000000000],
        results: [
            [0xbfc00000, 0xbff8000000000000],
            [0xbfc00000, 0xbff8000000000000],
            [0xbfc00000, 0xbff8000000000000],
        ],
        raised: 0,
    },
    // sqrt two
    DirectionCase {
        opcode: 0x51,
        left: [0x3f800000, 0x3ff0000000000000],
        right: [0x40000000, 0x4000000000000000],
        results: [
            [0x3fb504f3, 0x3ff6a09e667f3bcc],
            [0x3fb504f4, 0x3ff6a09e667f3bcd],
            [0x3fb504f3, 0x3ff6a09e667f3bcc],
        ],
        raised: 32,
    },
    // sqrt three
    DirectionCase {
        opcode: 0x51,
        left: [0x3f800000, 0x3ff0000000000000],
        right: [0x40400000, 0x4008000000000000],
        results: [
            [0x3fddb3d7, 0x3ffbb67ae8584caa],
            [0x3fddb3d8, 0x3ffbb67ae8584cab],
            [0x3fddb3d7, 0x3ffbb67ae8584caa],
        ],
        raised: 32,
    },
    // sqrt exact
    DirectionCase {
        opcode: 0x51,
        left: [0x3f800000, 0x3ff0000000000000],
        right: [0x40800000, 0x4010000000000000],
        results: [
            [0x40000000, 0x4000000000000000],
            [0x40000000, 0x4000000000000000],
            [0x40000000, 0x4000000000000000],
        ],
        raised: 0,
    },
];

fn directions() {
    for wide in [false, true] {
        for packed in [false, true] {
            for opcode in [0x58, 0x5c, 0x59, 0x5e, 0x51] {
                let instruction = encoding(prefix(wide, packed), opcode, 0xc1);
                let code = [&[0x90][..], &instruction].concat();
                for rc in 1..4 {
                    for masks in [0x1f80, 0x1fa0, 0x0fa0] {
                        let observed = floating_image(&code, masks | rc << 13);
                        let compiled = Compiler::new(SegmentProfile::Flat32)
                            .specialize_on_cpu(&observed.cpu)
                            .compile_block(0x1000, &code, 2)
                            .unwrap();
                        let module = TestModule::new(&compiled);
                        for case in CASES.iter().filter(|case| case.opcode == opcode) {
                            let mut image = floating_image(&code, masks | rc << 13);
                            let lanes = if wide { 2 } else { 4 };
                            let active = if packed { lanes } else { 1 };
                            for index in 0..lanes {
                                let bits = if index < active {
                                    ONE[wide as usize]
                                } else {
                                    SNAN[wide as usize]
                                };
                                lane(&mut image.cpu.simd.xmm[0], wide, index, bits);
                                lane(&mut image.cpu.simd.xmm[1], wide, index, bits);
                            }
                            lane(
                                &mut image.cpu.simd.xmm[0],
                                wide,
                                active - 1,
                                case.left[wide as usize],
                            );
                            lane(
                                &mut image.cpu.simd.xmm[1],
                                wide,
                                active - 1,
                                case.right[wide as usize],
                            );
                            let prior = retire(image.cpu, 1);
                            let fault = case.raised & !(masks >> 7) != 0;
                            let mut cpu = if fault {
                                prior
                            } else {
                                retire(prior, instruction.len())
                            };
                            cpu.simd.mxcsr |= case.raised;
                            if !fault {
                                let neutral = match opcode {
                                    0x58 => TWO,
                                    0x5c => ZERO,
                                    _ => ONE,
                                };
                                for index in 0..active {
                                    let bits = if index == active - 1 {
                                        case.results[(rc - 1) as usize][wide as usize]
                                    } else if opcode == 0x5c && rc == 1 {
                                        NEG_ZERO[wide as usize]
                                    } else {
                                        neutral[wide as usize]
                                    };
                                    lane(&mut cpu.simd.xmm[0], wide, index, bits);
                                }
                            }
                            let step = Step {
                                cpu,
                                ram: &[],
                                exit: if fault {
                                    Exit::SimdFloatingPoint
                                } else {
                                    Exit::Dispatch(cpu.eip)
                                },
                            };
                            assert_eq!(
                                module.observe( &image.input(), 1),
                                expected(&image, &[step]),
                                "opcode={opcode:x}, wide={wide}, packed={packed}, MXCSR={:x}, left={:x?}, right={:x?}",
                                image.cpu.simd.mxcsr, case.left, case.right
                            );
                        }
                    }
                }
            }
        }
    }
}

fn zeros() {
    for wide in [false, true] {
        for opcode in [0x58, 0x5c] {
            let code = encoding(prefix(wide, true), opcode, 0xc1);
            for rc in 0..4 {
                let observed = floating_image(&code, 0x1f80 | rc << 13);
                let compiled = Compiler::new(SegmentProfile::Flat32)
                    .specialize_on_cpu(&observed.cpu)
                    .compile_block(0x1000, &code, 1)
                    .unwrap();
                let module = TestModule::new(&compiled);
                for left_negative in [false, true] {
                    for right_negative in [false, true] {
                        let mut image = floating_image(&code, 0x1f80 | rc << 13);
                        let right_addend_negative = right_negative ^ (opcode == 0x5c);
                        let negative = if left_negative == right_addend_negative {
                            left_negative
                        } else {
                            rc == 1
                        };
                        for index in 0..if wide { 2 } else { 4 } {
                            lane(
                                &mut image.cpu.simd.xmm[0],
                                wide,
                                index,
                                if left_negative {
                                    NEG_ZERO[wide as usize]
                                } else {
                                    0
                                },
                            );
                            lane(
                                &mut image.cpu.simd.xmm[1],
                                wide,
                                index,
                                if right_negative {
                                    NEG_ZERO[wide as usize]
                                } else {
                                    0
                                },
                            );
                        }
                        let mut cpu = retire(image.cpu, code.len());
                        for index in 0..if wide { 2 } else { 4 } {
                            lane(
                                &mut cpu.simd.xmm[0],
                                wide,
                                index,
                                if negative { NEG_ZERO[wide as usize] } else { 0 },
                            );
                        }
                        assert_eq!(
                            module.observe(&image.input(), 1),
                            expected(&image, &[dispatch(cpu)])
                        );
                    }
                }
            }
        }
    }
}

fn restart() {
    let code = [0x90, 0xf3, 0x0f, 0x59, 0xc1]; // NOP; MULSS
    let mut image = floating_image(&code, 0x3f80);
    lane(&mut image.cpu.simd.xmm[0], false, 0, MIN_NORMAL[0]);
    lane(&mut image.cpu.simd.xmm[1], false, 0, 0x3f00_0000);
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .specialize_on_cpu(&image.cpu)
        .compile_block(0x1000, &code, 2)
        .unwrap();
    let module = TestModule::new(&compiled);
    let prior = retire(image.cpu, 1);
    assert_eq!(
        module.observe(&image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: prior,
                ram: &[],
                exit: Exit::Interpret,
            }]
        )
    );
    let mut cpu = retire(prior, 4);
    lane(&mut cpu.simd.xmm[0], false, 0, 0x0040_0000);
    let linked = module.with_interpreter(TestModule::interpreter());
    assert_eq!(
        linked.observe(&image.input(), 1),
        expected(&image, &[dispatch(cpu)])
    );
}

fn generic_blocks_restart_for_directed_rounding() {
    for wide in [false, true] {
        for packed in [false, true] {
            let lanes = if wide { 2 } else { 4 };
            let count = if packed { lanes } else { 1 };
            let code = encoding(prefix(wide, packed), 0x58, 0xc1);
            for rc in 1..4 {
                let mut image = floating_image(&code, 0x1f80 | rc << 13);
                for index in 0..count {
                    lane(&mut image.cpu.simd.xmm[0], wide, index, ONE[wide as usize]);
                    lane(
                        &mut image.cpu.simd.xmm[1],
                        wide,
                        index,
                        HALF_ULP[wide as usize],
                    );
                }
                let generic = Compiler::new(SegmentProfile::Flat32)
                    .compile_block(0x1000, &code, 1)
                    .unwrap();
                assert_eq!(
                    TestModule::new(&generic).observe(&image.input(), 1),
                    expected(
                        &image,
                        &[Step {
                            cpu: image.cpu,
                            ram: &[],
                            exit: Exit::Interpret
                        }]
                    )
                );
            }
        }
    }
}

#[test]
fn native_directed_arithmetic() {
    directions();
    zeros();
    restart();
    generic_blocks_restart_for_directed_rounding();
}
