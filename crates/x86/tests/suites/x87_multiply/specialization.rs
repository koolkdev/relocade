//! JIT guards retain dynamic controls and hand off before instruction effects.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use wasm86_x86::compile_block_from_bytes;

fn dynamic_controls(engine: Engine) {
    let code = [0xde, 0xc9];
    let module = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    // (1 + 2^-63) * 1.5 lies halfway between two extended values,
    // but below the first midpoint at either reduced precision.
    for (pc, unit, nearest, truncated) in [
        (0, 1_u64 << 40, 0, 0),
        (1, 1, 2, 1), // Reserved PC follows the existing full-precision policy.
        (2, 1 << 11, 0, 0),
        (3, 1, 2, 1),
    ] {
        for negative in [false, true] {
            for rc in 0..4 {
                for masked in [false, true] {
                    let mut image = stack_image(&code, 7, 0x3ffc);
                    image.cpu.x87.status.precision = 0;
                    set_control(
                        &mut image.cpu.x87.control,
                        0x005f | (pc << 8) | (rc << 10) | if masked { PE } else { 0 },
                    );
                    let sign = if negative { 0x8000 } else { 0 };
                    write_value(&mut image.cpu, 0, (LEADING + 1, 0x3fff | sign));
                    write_value(&mut image.cpu, 7, (0xc000_0000_0000_0000, 0x3fff));
                    let increment = match rc {
                        0 => nearest != truncated,
                        1 => negative,
                        2 => !negative,
                        _ => false,
                    };
                    let offset = if rc == 0 {
                        nearest
                    } else {
                        truncated + if increment { unit } else { 0 }
                    };
                    let mut result = complete_x87(image.cpu, 2, 0x06c9);
                    write_value(
                        &mut result,
                        0,
                        (0xc000_0000_0000_0000 + offset, 0x3fff | sign),
                    );
                    result.x87.tag_word |= 0xc000;
                    result.x87.status = status(
                        0x4500
                            | PE
                            | if increment { C1 } else { 0 }
                            | if masked { 0 } else { PENDING },
                    );
                    assert_eq!(
                        engine.observe(&module, &image.input(), 1),
                        expected(&image, &[dispatch(result)]),
                        "normal FMULP stays in the JIT: PC={pc} RC={rc} negative={negative} masked={masked}"
                    );
                }
            }
        }
    }
    for exponent in [1, 0x7ffe] {
        let mut image = stack_image(&code, 7, 0x3ffc);
        write_value(&mut image.cpu, 0, (LEADING, exponent));
        write_value(&mut image.cpu, 7, (LEADING, 0x3fff));
        let mut result = complete_x87(image.cpu, 2, 0x06c9);
        result.x87.status.top = 0;
        result.x87.status.c1 = 0;
        result.x87.tag_word |= 0xc000;
        assert_eq!(
            engine.observe(&module, &image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
}

fn signed_zero_products(engine: Engine) {
    let code = [0xde, 0xc9];
    let module = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    for (left, right) in [
        ((0, 0), (LEADING + 1, 0x3fff)),
        ((LEADING + 1, 0x3fff), (0, 0)),
        ((0, 0), (0, 0)),
    ] {
        for left_sign in [0, 0x8000] {
            for right_sign in [0, 0x8000] {
                for pc in 0..4 {
                    for rc in 0..4 {
                        for masked in [false, true] {
                            let mut image = stack_image(&code, 7, 0x3ffc);
                            // Masked sticky PE survives an exact product. Clear
                            // it before testing a control word with all masks off.
                            image.cpu.x87.status.precision = u8::from(masked);
                            set_control(
                                &mut image.cpu.x87.control,
                                (pc << 8) | (rc << 10) | if masked { 0x3f } else { 0 },
                            );
                            write_value(&mut image.cpu, 0, (left.0, left.1 | left_sign));
                            write_value(&mut image.cpu, 7, (right.0, right.1 | right_sign));
                            let mut product = complete_x87(image.cpu, 2, 0x06c9);
                            write_value(&mut product, 0, (0, left_sign ^ right_sign));
                            product.x87.status.top = 0;
                            product.x87.status.c1 = 0;
                            product.x87.tag_word |= 0xc000;
                            assert_eq!(
                                engine.observe(&module, &image.input(), 1),
                                expected(&image, &[dispatch(product)]),
                                "zero FMULP stays in the JIT: {left:x?} * {right:x?}, signs={left_sign:x}/{right_sign:x} PC={pc} RC={rc} masked={masked}"
                            );
                        }
                    }
                }
            }
        }
    }
    for exponent in [1, 0x7ffe] {
        let mut image = stack_image(&code, 7, 0x3ffc);
        write_value(&mut image.cpu, 0, (0, 0x8000));
        write_value(&mut image.cpu, 7, (u64::MAX, exponent));
        let mut product = complete_x87(image.cpu, 2, 0x06c9);
        product.x87.status.top = 0;
        product.x87.status.c1 = 0;
        product.x87.tag_word |= 0xc000;
        assert_eq!(
            engine.observe(&module, &image.input(), 1),
            expected(&image, &[dispatch(product)]),
            "zero times a normal value at exponent {exponent:x} is still exact"
        );
    }
}

fn rejected_products(engine: Engine) {
    let code = [0xde, 0xc9];
    let compiled = compile_block_from_bytes(0x1000, &code, 1).unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    let one = (LEADING, 0x3fff);
    for (left, right, result, flags) in [
        ((0, 0x8000), (LEADING, 0x7fff), INDEFINITE, 1),
        (
            (0, 0),
            (0xc000_0000_0000_0042, 0xffff),
            (0xc000_0000_0000_0042, 0xffff),
            0,
        ),
        ((0, 0), (1, 0x3fff), INDEFINITE, 1),
        ((0, 0x8000), (1, 0), (0, 0x8000), 2),
        ((0, 0), (LEADING, 0x8000), (0, 0x8000), 2),
        ((1, 0), one, (1, 0), 2),
        ((LEADING, 0), one, (LEADING, 1), 2),
        ((LEADING, 0x7fff), one, (LEADING, 0x7fff), 0),
        (
            (0xc000_0000_0000_0042, 0xffff),
            one,
            (0xc000_0000_0000_0042, 0xffff),
            0,
        ),
        (
            (LEADING + 1, 0x7fff),
            one,
            (0xc000_0000_0000_0001, 0x7fff),
            1,
        ),
        ((1, 0x3fff), one, INDEFINITE, 1),
        ((LEADING, 1), (LEADING, 0x3ffe), (LEADING >> 1, 0), 0),
        (
            (LEADING, 0x7ffe),
            (LEADING, 0x4000),
            (LEADING, 0x7fff),
            8 | PE | C1,
        ),
        // The unrounded exponent is in range; the rounding carry overflows.
        (
            (LEADING + 1, 0x7ffe),
            (u64::MAX - 1, 0x3fff),
            (LEADING, 0x7fff),
            8 | PE | C1,
        ),
        // The unrounded result is tiny even though precision rounding reaches normal.
        (
            (LEADING + 1, 1),
            (u64::MAX - 1, 0x3ffe),
            (LEADING, 1),
            PE | C1,
        ),
    ] {
        let mut image = stack_image(&code, 7, 0x3ffc);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, left);
        write_value(&mut image.cpu, 7, right);
        image.check_unchanged_exit(
            engine,
            &block,
            "restart before rejected FMULP",
            Exit::Interpret,
        );
        let mut product = complete_x87(image.cpu, 2, 0x06c9);
        write_value(&mut product, 0, result);
        product.x87.tag_word |= 0xc000;
        product.x87.status = status(0x4500 | flags);
        assert_eq!(
            engine.observe(&linked, &image.input(), 1),
            expected(&image, &[dispatch(product)]),
            "the interpreter completes rejected operands {left:x?} * {right:x?}"
        );
    }
    let mut image = stack_image(&code, 7, 0x3fff);
    // A zero payload does not establish that this destination is present.
    write_register_bits(&mut image.cpu, 0, (0, 0));
    write_value(&mut image.cpu, 7, one);
    image.check_unchanged_exit(
        engine,
        &block,
        "empty destination restarts before pop",
        Exit::Interpret,
    );
    let mut product = complete_x87(image.cpu, 2, 0x06c9);
    write_value(&mut product, 0, INDEFINITE);
    product.x87.tag_word |= 0xc000;
    product.x87.status = status(0x4561);
    assert_eq!(
        engine.observe(&linked, &image.input(), 1),
        expected(&image, &[dispatch(product)])
    );

    // Zero does not suppress an unmasked denormal-operand exception.
    let mut image = stack_image(&code, 7, 0x3ffc);
    set_control(&mut image.cpu.x87.control, 0x037d);
    write_value(&mut image.cpu, 0, (0, 0x8000));
    write_value(&mut image.cpu, 7, (1, 0));
    image.check_unchanged_exit(
        engine,
        &block,
        "denormal operand still restarts",
        Exit::Interpret,
    );
    let mut product = complete_x87(image.cpu, 2, 0x06c9);
    product.x87.status = status(0xfda2);
    assert_eq!(
        engine.observe(&linked, &image.input(), 1),
        expected(&image, &[dispatch(product)])
    );
}

fn live_zero_products(engine: Engine) {
    // A rounded normal result becomes -0, then either +0 or an invalid product.
    let code = [0xd8, 0xc9, 0xd8, 0xca, 0xd8, 0xcb];
    let compiled = compile_block_from_bytes(0x1000, &code, 3).unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    for infinity in [false, true] {
        let mut image = stack_image(&code, 0, 0xff00);
        image.cpu.x87.status.precision = 0;
        image.cpu.x87.status.c1 = 0;
        write_value(&mut image.cpu, 0, (LEADING + 1, 0x3fff));
        write_value(&mut image.cpu, 1, (0xc000_0000_0000_0000, 0x3fff));
        write_value(&mut image.cpu, 2, (0, 0x8000));
        write_value(
            &mut image.cpu,
            3,
            (LEADING, if infinity { 0x7fff } else { 0xbfff }),
        );
        let mut first = complete_x87(image.cpu, 2, 0x00c9);
        first.x87.status.precision = 1;
        first.x87.status.c1 = 1;
        write_value(&mut first, 0, (0xc000_0000_0000_0002, 0x3fff));
        let mut second = complete_x87(first, 2, 0x00ca);
        second.x87.status.c1 = 0;
        write_value(&mut second, 0, (0, 0x8000));
        let mut third = complete_x87(second, 2, 0x00cb);
        if infinity {
            assert_eq!(
                engine.observe(&block, &image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu: second,
                        ram: &[],
                        exit: Exit::Interpret
                    }]
                ),
                "handoff publishes the zero result, cleared C1 and sticky PE"
            );
            third.x87.status.invalid = 1;
            write_value(&mut third, 0, INDEFINITE);
        } else {
            write_value(&mut third, 0, (0, 0));
            assert_eq!(
                engine.observe(&block, &image.input(), 1),
                expected(&image, &[dispatch(third)]),
                "normal and zero products stay live in one JIT block"
            );
        }
        assert_eq!(
            engine.observe(&linked, &image.input(), 1),
            expected(&image, &[dispatch(third)])
        );
    }
}

fn prior_rounding_survives_handoff(engine: Engine) {
    // The second multiply is rejected without overwriting the first one's C1,
    // sticky PE, result, metadata, EIP or completed-instruction count.
    let code = [0xd8, 0xc9, 0xde, 0xca];
    let compiled = compile_block_from_bytes(0x1000, &code, 2).unwrap();
    let module = TestModule::new(&compiled);
    let mut image = stack_image(&code, 0, 0xffc0);
    image.cpu.x87.status.precision = 0;
    image.cpu.x87.status.c1 = 0;
    write_value(&mut image.cpu, 0, (LEADING + 1, 0x3fff));
    write_value(&mut image.cpu, 1, (0xc000_0000_0000_0000, 0x3fff));
    write_value(&mut image.cpu, 2, (LEADING, 0x7fff));
    let mut first = complete_x87(image.cpu, 2, 0x00c9);
    first.x87.status.precision = 1;
    first.x87.status.c1 = 1;
    write_value(&mut first, 0, (0xc000_0000_0000_0002, 0x3fff));
    assert_eq!(
        engine.observe(&module, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: first,
                ram: &[],
                exit: Exit::Interpret
            }]
        ),
    );
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    let mut second = complete_x87(first, 2, 0x06ca);
    second.x87.status.top = 1;
    second.x87.status.c1 = 0;
    second.x87.tag_word |= 3;
    assert_eq!(
        engine.observe(&linked, &image.input(), 1),
        expected(&image, &[dispatch(second)])
    );
}

#[test]
fn normal_products_keep_all_controls_dynamic() {
    dynamic_controls(Engine::Wasmtime);
}

#[test]
fn signed_zero_products_keep_all_controls_dynamic() {
    signed_zero_products(Engine::Wasmtime);
}

#[test]
fn live_zero_products_preserve_rounding_and_handoff_state() {
    live_zero_products(Engine::Wasmtime);
}

#[test]
fn rejected_products_restart_and_complete_in_the_interpreter() {
    rejected_products(Engine::Wasmtime);
}

#[test]
fn handoff_preserves_the_previous_rounding_state() {
    prior_rounding_survives_handoff(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_arithmetic_specialization_preserves_controls_and_restart_state() {
    dynamic_controls(Engine::V8);
    signed_zero_products(Engine::V8);
    live_zero_products(Engine::V8);
    rejected_products(Engine::V8);
    prior_rounding_survives_handoff(Engine::V8);
}
