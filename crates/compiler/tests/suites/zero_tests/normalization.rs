//! Normalizing the leading bit preserves zero only at the counted width.

use super::*;
use crate::wasm::{Input, Observation};

fn normalized32() -> TestModule {
    Fixture::new().function(&[Type::I32], &[Type::I1; 2], |body| {
        let input = body.parameter::<I32>(0)?;
        let normalized = input.shl(input.clz());
        body.return_((normalized.eq(0), normalized.ne(0)))
    })
}

fn normalized64() -> TestModule {
    Fixture::new().function(&[Type::I64], &[Type::I1; 2], |body| {
        let input = body.parameter::<I64>(0)?;
        let normalized = input.shl(input.clz().truncate::<I32>());
        body.return_((normalized.eq(0_u64), normalized.ne(0_u64)))
    })
}

#[test]
fn zero_tests_do_not_evaluate_leading_bit_normalization() {
    for module in [normalized32(), normalized64()] {
        for payload in Parser::new(0).parse_all(module.bytes()) {
            if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                for operator in body.get_operators_reader().unwrap() {
                    assert!(!matches!(
                        operator.unwrap(),
                        Operator::I32Clz | Operator::I64Clz | Operator::I32Shl | Operator::I64Shl
                    ));
                }
            }
        }
    }
    let mut narrow = normalized32().instantiate();
    let mut wide = normalized64().instantiate();
    for input in [0_i64, 1, 2, 0x8000_0000, 0x1_0000_0000, i64::MIN, -1] {
        let narrow_zero = i32::from(input as i32 == 0);
        let wide_zero = i32::from(input == 0);
        assert_eq!(
            narrow.call::<(i32, i32)>(input as i32).unwrap(),
            (narrow_zero, 1 - narrow_zero)
        );
        assert_eq!(
            wide.call::<(i32, i32)>(input).unwrap(),
            (wide_zero, 1 - wide_zero)
        );
    }
}

fn counterexamples() -> TestModule {
    Fixture::new().function(&[Type::I64, Type::I64], &[Type::I1; 5], |body| {
        let input = body.parameter::<I64>(0)?;
        let other = body.parameter::<I64>(1)?;
        body.return_([
            input.shl(other.clz().truncate::<I32>()).eq(0_u64),
            input.shl(input.truncate::<I32>().clz()).eq(0_u64),
            input
                .unsigned()
                .shr(input.clz().truncate::<I32>())
                .eq(0_u64),
            input
                .shl(input.clz().truncate::<I32>())
                .truncate::<I8>()
                .eq(0),
            input.shl(input.ctz().truncate::<I32>()).eq(0_u64),
        ])
    })
}

#[test]
fn other_inputs_widths_directions_and_result_masks_keep_their_zero_semantics() {
    let mut instance = counterexamples().instantiate();
    for (input, other, expected) in [
        (0_i64, 1_i64, [1, 1, 1, 1, 1]),
        (1, 1, [0, 0, 1, 1, 0]),
        (2, 1, [1, 0, 1, 1, 0]),
        (i64::MIN, 1, [1, 1, 0, 1, 1]),
        (-1, -1, [0, 0, 0, 0, 0]),
    ] {
        assert_eq!(
            instance
                .call_values("run", &[Value::I64(input), Value::I64(other)])
                .unwrap(),
            expected.map(Value::I32)
        );
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn normalization_zero_tests_execute_in_v8() {
    for (module, inputs) in [
        (normalized32(), [Value::I32(0), Value::I32(i32::MIN)]),
        (normalized64(), [Value::I64(0), Value::I64(i64::MIN)]),
    ] {
        for (input, expected) in inputs.into_iter().zip([[1, 0], [0, 1]]) {
            assert_eq!(
                module.run_v8(&Input::call("run", &[input])),
                Observation::returned(&expected.map(Value::I32))
            );
        }
    }
    assert_eq!(
        counterexamples().run_v8(&Input::call("run", &[Value::I64(i64::MIN), Value::I64(1)])),
        Observation::returned(&[1, 1, 0, 1, 1].map(Value::I32))
    );
}
