//! PC53/nearest quotients preserve precision evidence, x87 range and restart state.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use wasm86_x86::{BlockCompiler, CpuState};
use wasmparser::{Operator, Parser, Payload};

fn compiler() -> BlockCompiler {
    let mut observed = CpuState::default();
    observed.x87.control.precision_control = 2;
    BlockCompiler::new(SegmentProfile::Flat32).specialize_on_cpu(&observed)
}

fn native_module(code: &[u8], instruction_count: u32) -> TestModule {
    let module = TestModule::new(&compiler().compile(0x1000, code, instruction_count).unwrap());
    assert!(Parser::new(0).parse_all(module.bytes()).any(|payload| {
        matches!(payload.unwrap(), Payload::CodeSectionEntry(body) if body.get_operators_reader().unwrap()
            .into_iter().any(|op| matches!(op.unwrap(), Operator::F64Div)))
    }), "fixture must reach native division");
    module
}

fn rounding_forms_and_signed_zero(engine: Engine) {
    for (operation, destination, reverse, pop) in [
        ([0xd8, 0xf1], 7, false, false),
        ([0xd8, 0xf9], 7, true, false),
        ([0xde, 0xf9], 0, true, true),
    ] {
        let mut code = vec![0xdd, 0x05, 0, 0x40, 0, 0, 0xdd, 0x05, 8, 0x40, 0, 0];
        code.extend(operation);
        let module = native_module(&code, 3);
        for (numerator, denominator) in [
            (LEADING, LEADING),
            (LEADING, 0xc000_0000_0000_0000),
            (LEADING, 0xa000_0000_0000_0000),
            (0xffff_ffff_ffff_f800, LEADING),
            (LEADING, 0xffff_ffff_ffff_f800),
            (0, LEADING),
            (0xffff_ffff_ffff_f000, 0xffff_ffff_ffff_f800),
            (0xffff_ffff_ffff_f800, 0xffff_ffff_ffff_f000),
        ] {
            for numerator_sign in [0_u16, 0x8000] {
                for denominator_sign in [0_u16, 0x8000] {
                    let numerator_value = (
                        numerator,
                        if numerator == 0 { 0 } else { 0x3fff } | numerator_sign,
                    );
                    let denominator_value = (denominator, 0x3fff | denominator_sign);
                    let (left, right) = if reverse {
                        (denominator_value, numerator_value)
                    } else {
                        (numerator_value, denominator_value)
                    };
                    let mut image = stack_image(&code, 1, 0xffff);
                    set_control(&mut image.cpu.x87.control, 0x027f);
                    image.cpu.x87.status.precision = 0;
                    image.map(4, 0x8000, false);
                    let binary64 = |(significand, sign_exponent): (u64, u16)| {
                        (if significand == 0 {
                            0
                        } else {
                            0x3ff0_0000_0000_0000 | ((significand & (LEADING - 1)) >> 11)
                        }) | (u64::from(sign_exponent & 0x8000) << 48)
                    };
                    image.data(0x8000, &binary64(right).to_le_bytes());
                    image.data(0x8008, &binary64(left).to_le_bytes());
                    let mut result = complete_x87(image.cpu, 6, 0x0505);
                    result = complete_x87(result, 6, 0x0505);
                    result.x87.status.top = 7;
                    result.x87.data_offset = 0x4008;
                    result.x87.data_selector = 0x23;
                    write_value(&mut result, 0, right);
                    write_value(&mut result, 7, left);
                    result = complete_x87(
                        result,
                        2,
                        (u16::from(operation[0] & 7) << 8) | u16::from(operation[1]),
                    );
                    let (quotient, flags) = if numerator == 0 {
                        ((0, numerator_sign ^ denominator_sign), 0)
                    } else {
                        rounding::exact_quotient(
                            numerator,
                            denominator,
                            53,
                            0,
                            numerator_sign != denominator_sign,
                        )
                    };
                    write_value(&mut result, destination, quotient);
                    result.x87.status.precision = u8::from(flags & PE != 0);
                    result.x87.status.c1 = u8::from(flags & C1 != 0);
                    if pop {
                        result.x87.status.top = 0;
                        result.x87.tag_word |= 0xc000;
                    }
                    assert_eq!(
                        engine.observe(&module, &image.input(), 1),
                        expected(&image, &[dispatch(result)])
                    );
                }
            }
        }
    }
}

fn range_and_restarts(engine: Engine) {
    // The first division establishes PC53 precision for the native second division.
    let code = [0xdc, 0x35, 0, 0x40, 0, 0, 0xdc, 0x35, 8, 0x40, 0, 0];
    let module = native_module(&code, 2);
    let linked = TestModule::new(&compiler().compile(0x1000, &code, 2).unwrap())
        .with_interpreter(TestModule::interpreter());
    for (input, divisor, result_bits, flags, restart) in [
        (
            (LEADING, 1),
            0x3ff0_0000_0000_0000_u64,
            (LEADING, 1),
            0,
            true,
        ),
        (
            (LEADING, 1),
            0x4000_0000_0000_0000,
            (LEADING >> 1, 0),
            0,
            true,
        ),
        ((LEADING, 2), 0x3ff0_0000_0000_0000, (LEADING, 2), 0, false),
        (
            (LEADING, 0x3bcd),
            0x4000_0000_0000_0000,
            (LEADING, 0x3bcc),
            0,
            false,
        ),
        (
            (LEADING, 0x43fe),
            0x3fe0_0000_0000_0000,
            (LEADING, 0x43ff),
            0,
            false,
        ),
        (
            (LEADING, 0x7ffe),
            0x3ff0_0000_0000_0000,
            (LEADING, 0x7ffe),
            0,
            false,
        ),
        (
            (LEADING, 0x7ffe),
            0x3fe0_0000_0000_0000,
            (LEADING, 0x7fff),
            8 | PE | C1,
            true,
        ),
        ((LEADING, 0x3fff), 0, (LEADING, 0x7fff), 4, true),
        ((LEADING, 0x3fff), 1, (LEADING, 0x4431), 2, true),
        ((0, 0x8000), 0, INDEFINITE, 1, true),
        (
            (LEADING, 0x3fff),
            0x7ff0_0000_0000_0001,
            (0xc000_0000_0000_0800, 0x7fff),
            1,
            true,
        ),
    ] {
        let mut image = stack_image(&code, 0, 0xfffc);
        set_control(&mut image.cpu.x87.control, 0x027f);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, input);
        image.map(4, 0x8000, false);
        image.data(0x8000, &0x3ff0_0000_0000_0000_u64.to_le_bytes());
        image.data(0x8008, &divisor.to_le_bytes());
        let mut first = complete_x87(image.cpu, 6, 0x0435);
        first.x87.data_offset = 0x4000;
        first.x87.data_selector = 0x23;
        first.x87.status.c1 = 0;
        let mut result = complete_x87(first, 6, 0x0435);
        result.x87.data_offset = 0x4008;
        result.x87.status.invalid = u8::from(flags & 1 != 0);
        result.x87.status.denormal = u8::from(flags & 2 != 0);
        result.x87.status.zero_divide = u8::from(flags & 4 != 0);
        result.x87.status.overflow = u8::from(flags & 8 != 0);
        result.x87.status.precision = u8::from(flags & PE != 0);
        result.x87.status.c1 = u8::from(flags & C1 != 0);
        write_value(&mut result, 0, result_bits);
        let block_exit = if restart {
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
            expected(&image, &[block_exit]),
            "input={input:x?} divisor={divisor:x}"
        );
        if restart {
            assert_eq!(
                engine.observe(&linked, &image.input(), 1),
                expected(&image, &[dispatch(result)])
            );
        }
    }
}

fn arithmetic_precision_evidence(engine: Engine) {
    // Addition and cancellation produce the same PC53 evidence as multiplication.
    for (first_opcode, input, intermediate, quotient, flags) in [
        (
            0x05,
            (0, 0),
            (LEADING, 0x3fff),
            (0xcccc_cccc_cccc_d000, 0x3ffe),
            PE | C1,
        ),
        (0x25, (LEADING, 0x3fff), (0, 0), (0, 0), 0),
    ] {
        let code = [0xdc, first_opcode, 0, 0x40, 0, 0, 0xdc, 0x35, 8, 0x40, 0, 0];
        let module = native_module(&code, 2);
        for masked in [false, true] {
            let mut image = stack_image(&code, 0, 0xfffc);
            set_control(
                &mut image.cpu.x87.control,
                if masked { 0x027f } else { 0x025f },
            );
            image.cpu.x87.status.precision = 0;
            write_value(&mut image.cpu, 0, input);
            image.map(4, 0x8000, false);
            image.data(0x8000, &0x3ff0_0000_0000_0000_u64.to_le_bytes());
            image.data(0x8008, &0x3ff4_0000_0000_0000_u64.to_le_bytes());
            let mut result = complete_x87(image.cpu, 6, 0x0400 | u16::from(first_opcode));
            write_value(&mut result, 0, intermediate);
            result = complete_x87(result, 6, 0x0435);
            result.x87.data_offset = 0x4008;
            result.x87.data_selector = 0x23;
            result.x87.status.precision = u8::from(flags & PE != 0);
            result.x87.status.c1 = u8::from(flags & C1 != 0);
            if !masked && flags & PE != 0 {
                result.x87.status.error_summary = 1;
                result.x87.status.busy = 1;
            }
            write_value(&mut result, 0, quotient);
            assert_eq!(
                engine.observe(&module, &image.input(), 1),
                expected(&image, &[dispatch(result)])
            );
        }
    }
}

fn wider_operands_stay_in_jit(engine: Engine) {
    let code = [0xd8, 0xf1];
    let module = TestModule::new(&compiler().compile(0x1000, &code, 1).unwrap());
    let numerator = LEADING + 0x7ff;
    let denominator = LEADING + 1;
    let mut image = stack_image(&code, 0, 0xfff0);
    set_control(&mut image.cpu.x87.control, 0x027f);
    image.cpu.x87.status.precision = 0;
    write_value(&mut image.cpu, 0, (numerator, 0x3fff));
    write_value(&mut image.cpu, 1, (denominator, 0x3fff));
    let (quotient, flags) = rounding::exact_quotient(numerator, denominator, 53, 0, false);
    let mut result = complete_x87(image.cpu, 2, 0x00f1);
    result.x87.status.precision = u8::from(flags & PE != 0);
    result.x87.status.c1 = u8::from(flags & C1 != 0);
    write_value(&mut result, 0, quotient);
    assert_eq!(
        engine.observe(&module, &image.input(), 1),
        expected(&image, &[dispatch(result)])
    );
}

fn reverse_zero_divisor_restarts(engine: Engine) {
    let code = [0xdc, 0x35, 0, 0x40, 0, 0, 0xdc, 0x3d, 8, 0x40, 0, 0];
    let module = native_module(&code, 2);
    let linked = TestModule::new(&compiler().compile(0x1000, &code, 2).unwrap())
        .with_interpreter(TestModule::interpreter());
    let mut image = stack_image(&code, 0, 0xfffc);
    set_control(&mut image.cpu.x87.control, 0x027f);
    write_value(&mut image.cpu, 0, (0, 0x8000));
    image.map(4, 0x8000, false);
    image.data(0x8000, &0x3ff0_0000_0000_0000_u64.to_le_bytes());
    image.data(0x8008, &0x3ff4_0000_0000_0000_u64.to_le_bytes());
    let mut first = complete_x87(image.cpu, 6, 0x0435);
    first.x87.status.c1 = 0;
    first.x87.data_offset = 0x4000;
    first.x87.data_selector = 0x23;
    assert_eq!(
        engine.observe(&module, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: first,
                ram: &[],
                exit: Exit::Interpret
            }]
        )
    );
    let mut result = complete_x87(first, 6, 0x043d);
    result.x87.data_offset = 0x4008;
    result.x87.status.zero_divide = 1;
    write_value(&mut result, 0, (LEADING, 0xffff));
    assert_eq!(
        engine.observe(&linked, &image.input(), 1),
        expected(&image, &[dispatch(result)])
    );
}

#[test]
fn native_quotients_preserve_rounding_forms_and_signed_zero() {
    rounding_forms_and_signed_zero(Engine::Wasmtime);
}

#[test]
fn native_quotients_preserve_extended_range_and_restart() {
    range_and_restarts(Engine::Wasmtime);
    reverse_zero_divisor_restarts(Engine::Wasmtime);
}

#[test]
fn precision53_evidence_preserves_wider_operands_and_precision_exceptions() {
    arithmetic_precision_evidence(Engine::Wasmtime);
    wider_operands_stay_in_jit(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_native_quotients_preserve_rounding_forms_and_signed_zero() {
    rounding_forms_and_signed_zero(Engine::V8);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_native_quotients_preserve_extended_range_and_restart() {
    range_and_restarts(Engine::V8);
    reverse_zero_divisor_restarts(Engine::V8);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_precision53_evidence_preserves_wider_operands_and_precision_exceptions() {
    arithmetic_precision_evidence(Engine::V8);
    wider_operands_stay_in_jit(Engine::V8);
}
