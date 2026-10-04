//! PC53/nearest sums retain x87 signs, rounding status and exponent range.

use super::*;
use crate::support::{machine::expected, step::TestModule, x87::set_control};
use wasm86_x86::{BlockCompiler, CpuState};
use wasmparser::{Operator, Parser, Payload};

fn compiler() -> BlockCompiler {
    let mut observed = CpuState::default();
    observed.x87.control.precision_control = 2;
    BlockCompiler::new(SegmentProfile::Flat32).specialize_on_cpu(&observed)
}

fn native_module(code: &[u8], count: u32) -> TestModule {
    let module = TestModule::new(&compiler().compile(0x1000, code, count).unwrap());
    assert!(Parser::new(0).parse_all(module.bytes()).any(|payload| {
        matches!(payload.unwrap(), Payload::CodeSectionEntry(body) if body.get_operators_reader().unwrap()
            .into_iter().any(|op| matches!(op.unwrap(), Operator::F64Add)))
    }), "fixture must reach native addition");
    module
}

fn rounding_and_zero(engine: Engine) {
    for (operation, negate_left, negate_right, pop) in [
        ([0xd8, 0xc1], false, false, false),
        ([0xd8, 0xe1], false, true, false),
        ([0xd8, 0xe9], true, false, false),
        ([0xde, 0xe9], true, false, true),
    ] {
        let mut code = vec![0xdd, 0x05, 0, 0x40, 0, 0, 0xdd, 0x05, 8, 0x40, 0, 0];
        code.extend(operation);
        let module = native_module(&code, 3);
        for (left, right) in [
            (LEADING, LEADING),
            (LEADING, LEADING + 2048),
            (LEADING + 2048, LEADING),
            (LEADING + 2048, LEADING + 4096),
            (LEADING, 0xffff_ffff_ffff_f800),
            (0xffff_ffff_ffff_f800, LEADING),
            (0xffff_ffff_ffff_f800, 0xffff_ffff_ffff_f800),
            (0, LEADING),
            (LEADING, 0),
            (0, 0),
        ] {
            let gaps: &[u32] = if left == 0 || right == 0 {
                &[0]
            } else {
                &[0, 1, 2, 52, 53, 54, 55, 56, 63]
            };
            for &gap in gaps {
                for swap in [false, true] {
                    for left_negative in [false, true] {
                        for right_negative in [false, true] {
                            let left_sign = if left_negative { 0x8000 } else { 0 };
                            let right_sign = if right_negative { 0x8000 } else { 0 };
                            let left_value = (left, if left == 0 { 0 } else { 0x3fff } | left_sign);
                            let right_value = (
                                right,
                                if right == 0 { 0 } else { 0x3fff - gap as u16 } | right_sign,
                            );
                            let (left_value, right_value) = if swap {
                                (right_value, left_value)
                            } else {
                                (left_value, right_value)
                            };
                            let binary64 = |(significand, sign_exponent): (u64, u16)| {
                                (if significand == 0 {
                                    0
                                } else {
                                    (((i64::from(sign_exponent & 0x7fff) - 16383 + 1023) as u64)
                                        << 52)
                                        | ((significand & (LEADING - 1)) >> 11)
                                }) | (u64::from(sign_exponent & 0x8000) << 48)
                            };
                            let mut image = stack_image(&code, 1, 0xffff);
                            set_control(&mut image.cpu.x87.control, 0x027f);
                            image.cpu.x87.status.precision = 0;
                            image.map(4, 0x8000, false);
                            image.data(0x8000, &binary64(right_value).to_le_bytes());
                            image.data(0x8008, &binary64(left_value).to_le_bytes());
                            let mut result = complete_x87(image.cpu, 6, 0x0505);
                            result = complete_x87(result, 6, 0x0505);
                            result.x87.status.top = 7;
                            result.x87.data_offset = 0x4008;
                            result.x87.data_selector = 0x23;
                            write_value(&mut result, 0, right_value);
                            write_value(&mut result, 7, left_value);
                            result = complete_x87(
                                result,
                                2,
                                (u16::from(operation[0] & 7) << 8) | u16::from(operation[1]),
                            );
                            let high_negative =
                                left_negative ^ if swap { negate_right } else { negate_left };
                            let low_negative =
                                right_negative ^ if swap { negate_left } else { negate_right };
                            let (sum, flags) = if left == 0 && right == 0 {
                                (
                                    (
                                        0,
                                        if high_negative && low_negative {
                                            0x8000
                                        } else {
                                            0
                                        },
                                    ),
                                    0,
                                )
                            } else {
                                rounding::exact_sum(
                                    left,
                                    right,
                                    gap,
                                    high_negative,
                                    low_negative,
                                    53,
                                    0,
                                )
                            };
                            write_value(&mut result, if pop { 0 } else { 7 }, sum);
                            result.x87.status.precision = u8::from(flags & PE != 0);
                            result.x87.status.c1 = u8::from(flags & C1 != 0);
                            if pop {
                                result.x87.status.top = 0;
                                result.x87.tag_word |= 0xc000;
                            }
                            assert_eq!(
                            engine.observe(&module, &image.input(), 1),
                            expected(&image, &[dispatch(result)]),
                            "operation={operation:x?} left={left_value:x?} right={right_value:x?}"
                        );
                        }
                    }
                }
            }
        }
    }
}

fn range_and_restart(engine: Engine) {
    // Multiplication by a loaded one establishes PC53 for both wide-exponent registers.
    for (opcode, left, right, sum, flags, restart) in [
        (
            0xc1,
            (LEADING, 0x43ff),
            (LEADING, 0x43ff),
            (LEADING, 0x4400),
            0,
            false,
        ),
        (
            0xc1,
            (LEADING, 0x7ffe),
            (LEADING, 2),
            (LEADING, 0x7ffe),
            PE,
            false,
        ),
        (
            0xe1,
            (LEADING, 2),
            (LEADING, 0x7ffe),
            (LEADING, 0x7ffe),
            PE | C1,
            false,
        ),
        (0xe1, (LEADING, 2), (LEADING, 2), (0, 0), 0, false),
        (0xe1, (LEADING, 2), (LEADING + 2048, 2), (4096, 0), 0, true),
        (
            0xe1,
            (LEADING, 2),
            (0xc000_0000_0000_0000, 2),
            (LEADING, 1),
            0,
            true,
        ),
        (
            0xc1,
            (LEADING, 0x7ffe),
            (LEADING, 0x7ffe),
            (LEADING, 0x7fff),
            8 | PE | C1,
            true,
        ),
    ] {
        let code = [
            0xdc, 0x0d, 0, 0x40, 0, 0, 0xd9, 0xc9, 0xdc, 0x0d, 0, 0x40, 0, 0, 0xd8, opcode,
        ];
        let module = native_module(&code, 4);
        let linked = TestModule::new(&compiler().compile(0x1000, &code, 4).unwrap())
            .with_interpreter(TestModule::interpreter());
        let mut image = stack_image(&code, 0, 0xfff0);
        set_control(&mut image.cpu.x87.control, 0x027f);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, left);
        write_value(&mut image.cpu, 1, right);
        image.map(4, 0x8000, false);
        image.data(0x8000, &0x3ff0_0000_0000_0000_u64.to_le_bytes());
        let mut first = complete_x87(image.cpu, 6, 0x040d);
        first = complete_x87(first, 2, 0x01c9);
        first = complete_x87(first, 6, 0x040d);
        first.x87.data_offset = 0x4000;
        first.x87.data_selector = 0x23;
        first.x87.status.c1 = 0;
        write_value(&mut first, 0, right);
        write_value(&mut first, 1, left);
        let mut result = complete_x87(first, 2, u16::from(opcode));
        write_value(&mut result, 0, sum);
        result.x87.status.precision = u8::from(flags & PE != 0);
        result.x87.status.c1 = u8::from(flags & C1 != 0);
        result.x87.status.overflow = u8::from(flags & 8 != 0);
        let step = if restart {
            Step {
                cpu: first,
                ram: &[],
                exit: Exit::Interpret,
            }
        } else {
            dispatch(result)
        };
        assert_eq!(
            engine.observe(&module, &image.input(), 1),
            expected(&image, &[step])
        );
        if restart {
            assert_eq!(
                engine.observe(&linked, &image.input(), 1),
                expected(&image, &[dispatch(result)])
            );
        }
    }
}

fn precision_exception(engine: Engine) {
    let code = [0xdc, 0x0d, 0, 0x40, 0, 0, 0xdc, 0x05, 8, 0x40, 0, 0];
    let module = native_module(&code, 2);
    for (input, sum, c1) in [(LEADING, LEADING, 0), (LEADING + 2048, LEADING + 4096, 1)] {
        let mut image = stack_image(&code, 0, 0xfffc);
        set_control(&mut image.cpu.x87.control, 0x025f);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, (input, 0x3fff));
        image.map(4, 0x8000, false);
        image.data(0x8000, &0x3ff0_0000_0000_0000_u64.to_le_bytes());
        image.data(0x8008, &0x3ca0_0000_0000_0000_u64.to_le_bytes());
        let mut result = complete_x87(image.cpu, 6, 0x040d);
        result = complete_x87(result, 6, 0x0405);
        result.x87.data_offset = 0x4008;
        result.x87.data_selector = 0x23;
        result.x87.status.precision = 1;
        result.x87.status.error_summary = 1;
        result.x87.status.busy = 1;
        result.x87.status.c1 = c1;
        write_value(&mut result, 0, (sum, 0x3fff));
        assert_eq!(
            engine.observe(&module, &image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
}

#[test]
fn native_sums_preserve_rounding_and_signed_zero() {
    rounding_and_zero(Engine::Wasmtime);
}
#[test]
fn native_sums_preserve_extended_range_and_restart() {
    range_and_restart(Engine::Wasmtime);
}
#[test]
fn native_sums_commit_unmasked_precision() {
    precision_exception(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_native_sums_preserve_rounding_and_signed_zero() {
    rounding_and_zero(Engine::V8);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_native_sums_preserve_extended_range_and_restart() {
    range_and_restart(Engine::V8);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_native_sums_commit_unmasked_precision() {
    precision_exception(Engine::V8);
}
