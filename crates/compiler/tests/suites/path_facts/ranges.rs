//! Unsigned guards constrain later comparisons without narrowing stored values.

use super::*;
use wasm86_compiler::{Val, I64};

#[path = "ranges/control.rs"]
mod control;

fn interval() -> TestModule {
    Fixture::new().function(&[Type::I64], &[Type::I32], |mut body| {
        let input = body.parameter::<I64>(0)?;
        body.if_(
            input.unsigned().ge(10_u64).and(input.unsigned().lt(20_u64)),
            |arm| {
                arm.return_(
                    input
                        .unsigned()
                        .ge(5_u64)
                        .unsigned()
                        .extend::<I32>()
                        .or(input
                            .unsigned()
                            .lt(25_u64)
                            .unsigned()
                            .extend::<I32>()
                            .shl(1))
                        .or(input.eq(0_u64).unsigned().extend::<I32>().shl(2))
                        .or(input.ne(30_u64).unsigned().extend::<I32>().shl(3)),
                )
            },
        )?;
        body.return_(input.unsigned().lt(15_u64).select::<I32>(100, 200))
    })
}

#[test]
fn rewritten_operands_reuse_the_range_of_an_available_expression() {
    let module = Fixture::new().function(&[Type::I64], &[Type::I32], |mut body| {
        let input = body.parameter::<I64>(0)?;
        let low_byte = input.and(0xff_u64);
        let in_range = low_byte
            .unsigned()
            .ge(16_u64)
            .and(low_byte.unsigned().lt(32_u64));
        let needs_patch = low_byte.unsigned().ge(64_u64);
        let patched = input.or(needs_patch.select(0x80_u64, 0_u64));
        let masked_patch = patched.and(0xff_u64);
        body.if_(in_range.eq(false), |arm| arm.return_(7))?;
        body.return_(
            masked_patch
                .unsigned()
                .lt(16_u64)
                .unsigned()
                .extend::<I32>(),
        )
    });
    assert_eq!(
        count(&module, |op| matches!(
            op,
            Operator::I64GeU | Operator::I64LtU
        )),
        2
    );
    let mut instance = module.instantiate();
    for (input, expected) in [(0_u64, 7), (16, 0), (31, 0), (0x11f, 0), (64, 7)] {
        assert_eq!(
            instance.call_values("run", &[Value::I64(input as i64)]),
            Ok(vec![Value::I32(expected)])
        );
    }
}

#[test]
fn unsigned_intervals_fold_comparisons_only_inside_the_guard() {
    let module = interval();
    assert_eq!(
        count(&module, |op| matches!(
            op,
            Operator::I64GeU | Operator::I64LtU
        )),
        3
    );
    let mut instance = module.instantiate();
    for input in [0_u64, 5, 9, 10, 14, 15, 19, 20, 25, 30, 1 << 63, u64::MAX] {
        let expected = if (10..20).contains(&input) {
            11
        } else if input < 15 {
            100
        } else {
            200
        };
        assert_eq!(
            instance.call_values("run", &[Value::I64(input as i64)]),
            Ok(vec![Value::I32(expected)])
        );
    }
}

fn narrowed_input() -> TestModule {
    Fixture::new().function(&[Type::I32], &[Type::I32; 3], |mut body| {
        let original = body.parameter::<I32>(0)?;
        let byte = original.add(250).truncate::<I8>();
        body.if_(byte.unsigned().ge(240).eq(false), |arm| {
            arm.return_((
                byte.unsigned().lt(250).unsigned().extend::<I32>(),
                byte.unsigned().extend::<I32>(),
                &original,
            ))
        })?;
        body.return_((
            byte.unsigned().ge(230).unsigned().extend::<I32>(),
            byte.unsigned().extend::<I32>(),
            original,
        ))
    })
}

#[test]
fn range_facts_observe_logical_width_and_preserve_original_carriers() {
    let module = narrowed_input();
    assert_eq!(
        count(&module, |op| matches!(
            op,
            Operator::I32GeU | Operator::I32LtU
        )),
        1
    );
    let mut instance = module.instantiate();
    for input in [
        0_i32,
        1,
        5,
        6,
        230,
        245,
        255,
        256,
        511,
        i32::MIN,
        i32::MAX,
        -1,
    ] {
        let byte = input.wrapping_add(250) as u8;
        assert_eq!(
            instance.call_values("run", &[Value::I32(input)]),
            Ok(vec![
                Value::I32(1),
                Value::I32(i32::from(byte)),
                Value::I32(input)
            ])
        );
    }
}

fn dynamic_endpoints() -> TestModule {
    Fixture::new().function(&[Type::I32; 2], &[Type::I32; 2], |mut body| {
        let a = body.parameter::<I32>(0)?;
        let b = body.parameter::<I32>(1)?;
        body.if_(a.unsigned().lt(&b), |arm| {
            arm.return_((
                a.ne(u32::MAX).unsigned().extend::<I32>(),
                b.ne(0).unsigned().extend::<I32>(),
            ))
        })?;
        body.return_((
            a.unsigned().ge(&b).unsigned().extend::<I32>(),
            a.signed().lt(b).unsigned().extend::<I32>(),
        ))
    })
}

fn nonzero_endpoint() -> TestModule {
    Fixture::new().function(&[Type::I32], &[Type::I32], |mut body| {
        let input = body.parameter::<I32>(0)?;
        body.if_(Val::<I32>::from(0).unsigned().lt(&input), |arm| {
            arm.return_(input.unsigned().lt(1).select::<I32>(99, 7))
        })?;
        body.return_(input.unsigned().lt(1).select::<I32>(11, 99))
    })
}

#[test]
fn comparisons_canonicalized_to_zero_tests_keep_their_unsigned_bound() {
    let module = nonzero_endpoint();
    assert_eq!(count(&module, |op| matches!(op, Operator::I32LtU)), 0);
    let mut instance = module.instantiate();
    for (input, expected) in [(0, 11), (1, 7), (i32::MIN, 7), (-1, 7)] {
        assert_eq!(instance.call::<i32>((input,)), Ok(expected));
    }
}

#[test]
fn dynamic_unsigned_bounds_preserve_strict_endpoints_and_signed_comparisons() {
    let module = dynamic_endpoints();
    let mut instance = module.instantiate();
    let values = [0_u32, 1, 17, i32::MAX as u32, 1 << 31, u32::MAX];
    for a in values {
        for b in values {
            let expected = if a < b {
                [1, 1]
            } else {
                [1, i32::from((a as i32) < (b as i32))]
            };
            assert_eq!(
                instance.call_values("run", &[Value::I32(a as i32), Value::I32(b as i32)]),
                Ok(expected.map(Value::I32).to_vec())
            );
        }
    }
}

#[test]
#[ignore = "requires Node.js with V8"]
fn unsigned_range_specialization_executes_in_v8() {
    let module = interval();
    for (input, result) in [(9, 100), (10, 11), (19, 11), (20, 200), (-1, 200)] {
        check_result(&module, &[Value::I64(input)], &[Value::I32(result)], true);
    }
    let module = narrowed_input();
    for input in [0_i32, 6, 245, i32::MIN, -1] {
        check_result(
            &module,
            &[Value::I32(input)],
            &[
                Value::I32(1),
                Value::I32(i32::from(input.wrapping_add(250) as u8)),
                Value::I32(input),
            ],
            true,
        );
    }
    let module = nonzero_endpoint();
    for (input, result) in [(0, 11), (1, 7), (-1, 7)] {
        check_result(&module, &[Value::I32(input)], &[Value::I32(result)], true);
    }
}

#[test]
fn nested_intervals_include_swapped_and_extreme_endpoints() {
    let module = Fixture::new().function(&[Type::I64], &[Type::I32], |mut body| {
        let input = body.parameter::<I64>(0)?;
        body.if_(Val::<I64>::from(10_u64).unsigned().ge(&input), |arm| {
            arm.return_(1)
        })?;
        body.if_(Val::<I64>::from(20_u64).unsigned().lt(&input), |mut arm| {
            arm.if_(input.unsigned().lt(u64::MAX), |arm| {
                arm.return_(input.ne(u64::MAX).select::<I32>(2, 99))
            })?;
            arm.return_(input.eq(u64::MAX).select::<I32>(3, 99))
        })?;
        body.return_(
            input
                .unsigned()
                .ge(11_u64)
                .and(input.unsigned().lt(21_u64))
                .select::<I32>(4, 99),
        )
    });
    let mut instance = module.instantiate();
    for (input, expected) in [
        (0_u64, 1),
        (10, 1),
        (11, 4),
        (20, 4),
        (21, 2),
        (u64::MAX - 1, 2),
        (u64::MAX, 3),
    ] {
        assert_eq!(
            instance.call_values("run", &[Value::I64(input as i64)]),
            Ok(vec![Value::I32(expected)])
        );
    }
}

#[test]
fn a_truncated_range_does_not_narrow_the_original_value_or_its_signed_view() {
    let module = Fixture::new().function(&[Type::I32], &[Type::I32; 3], |mut body| {
        let input = body.parameter::<I32>(0)?;
        let byte = input.truncate::<I8>();
        body.if_(byte.unsigned().lt(16), |arm| {
            arm.return_((
                input.unsigned().lt(16).unsigned().extend::<I32>(),
                input
                    .and(0xffff)
                    .unsigned()
                    .lt(16)
                    .unsigned()
                    .extend::<I32>(),
                byte.signed().extend::<I32>(),
            ))
        })?;
        body.return_((0, 0, byte.signed().extend::<I32>()))
    });
    let mut instance = module.instantiate();
    for (input, expected) in [
        (7, [1, 1, 7]),
        (0x107, [0, 0, 7]),
        (0x10007, [0, 1, 7]),
        (0x80, [0, 0, -128]),
        (-1, [0, 0, -1]),
    ] {
        assert_eq!(
            instance.call_values("run", &[Value::I32(input)]),
            Ok(expected.map(Value::I32).to_vec())
        );
    }
}
