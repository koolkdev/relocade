//! Boolean values retain canonical numeric results through folding and type views.

use super::*;

fn count(module: &TestModule, predicate: impl Fn(&Operator<'_>) -> bool) -> usize {
    Validator::new().validate_all(module.bytes()).unwrap();
    Parser::new(0)
        .parse_all(module.bytes())
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(body),
            _ => None,
        })
        .map(|body| {
            body.get_operators_reader()
                .unwrap()
                .into_iter()
                .map(Result::unwrap)
                .filter(&predicate)
                .count()
        })
        .sum()
}

fn check(module: &TestModule, inputs: &[Value], expected: &[Value]) {
    assert_eq!(
        module.instantiate().call_values("run", inputs).unwrap(),
        expected
    );
}

#[test]
fn zero_one_choices_fold_before_and_after_specialization() {
    let module = Fixture::new().function(&[Type::I32], &[Type::I8, Type::I32, Type::I64], |body| {
        let condition = body.parameter::<I32>(0)?.truncate::<I1>().add(true);
        body.return_((
            condition.select::<I8>(1, 0),
            condition.select::<I32>(0, 1),
            condition.select::<I64>(1_u64, 0_u64),
        ))
    });
    assert_eq!(count(&module, |op| matches!(op, Operator::Select)), 0);
    for (input, expected) in [(0, 1), (1, 0), (2, 1), (3, 0), (-1, 0), (i32::MIN, 1)] {
        check(
            &module,
            &[Value::I32(input)],
            &[
                Value::I32(expected),
                Value::I32(1 - expected),
                Value::I64(i64::from(expected)),
            ],
        );
    }

    let specialized = Fixture::new().function(&[Type::I32, Type::I1], &[Type::I32], |mut body| {
        let input = body.parameter::<I32>(0)?;
        let enabled = body.parameter::<I1>(1)?;
        let numeric_bit = input
            .ne(0)
            .select::<I32>(enabled.select(1, 2), enabled.select(0, 3));
        body.if_(enabled, |arm| arm.return_(numeric_bit))?;
        body.return_(9)
    });
    assert_eq!(count(&specialized, |op| matches!(op, Operator::Select)), 0);
    for (input, enabled, expected) in [(0, 1, 0), (2, 1, 1), (-1, 1, 1), (2, 0, 9)] {
        check(
            &specialized,
            &[Value::I32(input), Value::I32(enabled)],
            &[Value::I32(expected)],
        );
    }
}

#[test]
fn complementary_predicates_fold_without_treating_wide_values_as_booleans() {
    let predicates = Fixture::new().function(&[Type::I64], &[Type::I1; 6], |body| {
        let zero = body.parameter::<I64>(0)?.eq(0_u64);
        let nonzero = zero.eq(false);
        body.return_([
            zero.and(&nonzero),
            nonzero.and(&zero),
            zero.or(&nonzero),
            nonzero.or(&zero),
            zero.xor(&nonzero),
            nonzero.xor(&zero),
        ])
    });
    assert_eq!(
        count(&predicates, |op| matches!(
            op,
            Operator::I32And | Operator::I32Or | Operator::I32Xor
        )),
        0
    );
    for input in [0, 1, 0x1_0000_0000, i64::MIN, -1] {
        check(
            &predicates,
            &[Value::I64(input)],
            &[0, 0, 1, 1, 1, 1].map(Value::I32),
        );
    }

    for bounded in [false, true] {
        let module = Fixture::new().function(&[Type::I32], &[Type::I32; 3], |body| {
            let input = body.parameter::<I32>(0)?;
            let value = if bounded { input.and(1) } else { input };
            let opposite = value.eq(0).unsigned().extend::<I32>();
            body.return_([
                value.and(&opposite),
                opposite.or(&value),
                value.xor(opposite),
            ])
        });
        if bounded {
            assert_eq!(
                count(&module, |op| matches!(
                    op,
                    Operator::I32And | Operator::I32Or | Operator::I32Xor
                )),
                0
            );
        }
        for (input, wide_expected) in [(0, 1), (1, 1), (2, 2), (i32::MIN, i32::MIN), (-1, -1)] {
            let expected = if bounded { 1 } else { wide_expected };
            check(
                &module,
                &[Value::I32(input)],
                &[0, expected, expected].map(Value::I32),
            );
        }
    }
}

#[test]
fn nested_zero_tests_keep_polarity_and_observe_the_full_input_width() {
    let module = Fixture::new().expression(&[Type::I64], |body| {
        body.parameter::<I64>(0)
            .unwrap()
            .eq(0_u64)
            .eq(false)
            .eq(false)
    });
    assert_eq!(
        count(&module, |op| matches!(
            op,
            Operator::I32Eqz | Operator::I64Eqz
        )),
        1
    );
    for (input, expected) in [(0, 1), (1, 0), (0x1_0000_0000, 0), (i64::MIN, 0), (-1, 0)] {
        check(&module, &[Value::I64(input)], &[Value::I32(expected)]);
    }
}
