//! Guarded calculations retain the identities of their computed inputs.
use super::*;
use wasm86_compiler::I16;

fn guarded_masks(v8: bool) {
    let module = Fixture::new().function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let invalid = body.parameter::<I1>(0)?;
        let input = body.parameter::<I32>(1)?.or(0x8000);
        let word = input.truncate::<I16>();
        let guarded = word.and(0x7fff);
        let conditional = invalid.select(0xffff, &word).and(0x7fff);
        body.if_(invalid, |arm| arm.return_(7))?;
        body.if_(guarded.eq(0), |arm| arm.return_(11))?;
        body.return_(conditional.eq(0).select(input.clz(), &input))
    });
    assert_eq!(count(&module, |op| matches!(op, Operator::I32Clz)), 0);
    for input in [0, 1, 32767, 32768, 65536, -1, i32::MIN, i32::MAX] {
        for invalid in [0, 1] {
            let expected = if invalid != 0 {
                7
            } else if input & 0x7fff == 0 {
                11
            } else {
                input | 0x8000
            };
            check_result(
                &module,
                &[Value::I32(invalid), Value::I32(input)],
                &[Value::I32(expected)],
                v8,
            );
        }
    }
}

fn computed_inputs(v8: bool) {
    let module = Fixture::new().function(
        &[Type::I1, Type::I1, Type::I32, Type::I32],
        &[Type::I32],
        |mut body| {
            let mode = body.parameter::<I1>(0)?;
            let flag = body.parameter::<I1>(1)?;
            let input = body.parameter::<I32>(2)?;
            let factor = body.parameter::<I32>(3)?;
            let offset = mode.select(input.add(1), input.add(2));
            let product = flag.select(offset, 7).mul(factor);
            body.if_(mode, |arm| arm.return_(0))?;
            body.if_(product.eq(0), |arm| arm.return_(1))?;
            body.if_(flag, |arm| arm.return_(product.add(1)))?;
            body.return_(2)
        },
    );
    assert_eq!(count(&module, |op| matches!(op, Operator::I32Mul)), 1);
    for (input, factor) in [(3_i32, 4_i32), (-2, 5), (7, 0), (i32::MAX, -1)] {
        for mode in [0, 1] {
            for flag in [0, 1] {
                let product =
                    if flag != 0 { input.wrapping_add(2) } else { 7 }.wrapping_mul(factor);
                let expected = if mode != 0 {
                    0
                } else if product == 0 {
                    1
                } else if flag != 0 {
                    product.wrapping_add(1)
                } else {
                    2
                };
                check_result(
                    &module,
                    &[
                        Value::I32(mode),
                        Value::I32(flag),
                        Value::I32(input),
                        Value::I32(factor),
                    ],
                    &[Value::I32(expected)],
                    v8,
                );
            }
        }
    }
}

fn switched_byte(v8: bool) {
    let module = Fixture::new().function(
        &[Type::I1, Type::I32],
        &[Type::I32, Type::I32],
        |mut body| {
            let mode = body.parameter::<I1>(0)?;
            let input = body.parameter::<I32>(1)?;
            let offset = mode.select(input.add(256), input.add(512));
            let byte = offset.truncate::<I8>();
            body.if_(mode, |arm| arm.return_((&input, 7)))?;
            body.switch(&byte, &[0, 129, 255], |arm, key| {
                let result = match key {
                    Some(_) => byte.signed().extend::<I32>(),
                    None => byte.unsigned().extend::<I32>(),
                };
                arm.return_((&offset, result))
            })?;
            body.return_((0, 0))
        },
    );
    for input in [0_i32, 1, 129, 255, 256, 385, -1, i32::MIN, i32::MAX] {
        for mode in [0, 1] {
            let offset = input.wrapping_add(512);
            let byte = offset as u8;
            let expected = if mode != 0 {
                [input, 7]
            } else {
                [
                    offset,
                    if matches!(byte, 0 | 129 | 255) {
                        i32::from(byte as i8)
                    } else {
                        i32::from(byte)
                    },
                ]
            };
            check_result(
                &module,
                &[Value::I32(mode), Value::I32(input)],
                &expected.map(Value::I32),
                v8,
            );
        }
    }
}

#[test]
fn masks_exposed_by_specialization_reuse_the_guarded_value() {
    guarded_masks(false);
}

#[test]
fn computed_inputs_survive_specialization_in_later_guards() {
    computed_inputs(false);
}

#[test]
fn switch_facts_preserve_wider_sources_of_specialized_bytes() {
    switched_byte(false);
}

#[test]
#[ignore = "requires Node.js with V8"]
fn v8_materialized_values_preserve_branch_results() {
    guarded_masks(true);
    computed_inputs(true);
    switched_byte(true);
}
