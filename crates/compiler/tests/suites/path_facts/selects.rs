use super::*;
use wasm86_compiler::I64;

fn correlated_fields(independent: bool) -> TestModule {
    Fixture::new().function(
        &[Type::I1, Type::I1, Type::I64, Type::I64],
        &[Type::I1],
        |body| {
            let first = body.parameter::<I1>(0)?;
            let second = if independent {
                body.parameter::<I1>(1)?
            } else {
                first.clone()
            };
            let payload = body.parameter::<I64>(2)?.unsigned().shr(1);
            let exponent = body.parameter::<I64>(3)?;
            let significand = payload.or(first.select(0_u64, 1_u64 << 63));
            let exponent = second.select(0_u64, exponent);
            body.return_(
                exponent
                    .ne(0_u64)
                    .and(significand.and(1_u64 << 63).eq(0_u64)),
            )
        },
    )
}

fn check_correlated_fields(v8: bool) {
    for independent in [false, true] {
        let module = correlated_fields(independent);
        if !independent {
            assert_eq!(
                count(&module, |op| matches!(op, Operator::LocalGet { .. })),
                0
            );
        }
        for (first, second, payload, exponent) in [
            (0, 0, 0, 0),
            (0, 1, -1, 1),
            (1, 0, i64::MIN, 1),
            (1, 1, i64::MAX, -1),
        ] {
            let expected = independent && first != 0 && second == 0 && exponent != 0;
            check_result(
                &module,
                &[
                    Value::I32(first),
                    Value::I32(second),
                    Value::I64(payload),
                    Value::I64(exponent),
                ],
                &[Value::I32(i32::from(expected))],
                v8,
            );
        }
    }
}

#[test]
fn predicates_correlate_fields_selected_by_the_same_condition() {
    check_correlated_fields(false);
}

#[test]
#[ignore = "requires Node.js with V8"]
fn v8_predicates_correlate_fields_selected_by_the_same_condition() {
    check_correlated_fields(true);
}
