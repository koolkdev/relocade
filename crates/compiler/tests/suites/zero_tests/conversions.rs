use super::{Fixture, Operator, Parser, Payload, TestModule, Type, I1, I32, I64, I8};
use crate::wasm::{Input, Observation, Value};

fn preserves_zero() -> TestModule {
    Fixture::new().function(&[Type::I64], &[Type::I1; 3], |body| {
        let input = body.parameter::<I64>(0)?;
        let bounded = input.unsigned().shr(32).truncate::<I32>();
        let narrow = input.truncate::<I8>().add(1).unsigned().extend::<I64>();
        body.return_((bounded.ne(0), bounded.eq(0), narrow.eq(0)))
    })
}

fn wraps(module: &TestModule) -> usize {
    Parser::new(0)
        .parse_all(module.bytes())
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(
                body.get_operators_reader()
                    .unwrap()
                    .into_iter()
                    .filter(|op| matches!(op.as_ref().unwrap(), Operator::I32WrapI64))
                    .count(),
            ),
            _ => None,
        })
        .sum()
}

#[test]
fn zero_tests_bypass_lossless_carrier_changes_but_keep_logical_masks() {
    let module = preserves_zero();
    // Only the independent byte arithmetic still needs an i32 carrier.
    assert_eq!(wraps(&module), 1);
    let mut instance = module.instantiate();
    for (input, expected) in [
        (0_i64, (0, 1, 0)),
        (255, (0, 1, 1)),
        (0x1_0000_0000, (1, 0, 0)),
        (-1, (1, 0, 1)),
    ] {
        assert_eq!(instance.call::<(i32, i32, i32)>(input).unwrap(), expected);
    }
    let lossy = Fixture::new().expression(&[Type::I64], |body| {
        body.parameter::<I64>(0).unwrap().truncate::<I32>().ne(0)
    });
    assert_eq!(wraps(&lossy), 1);
    let mut instance = lossy.instantiate();
    for (input, expected) in [(0_i64, 0), (0x1_0000_0000, 0), (0x1_0000_0001, 1), (-1, 1)] {
        assert_eq!(instance.call::<i32>(input).unwrap(), expected);
    }
}

#[test]
fn specialization_can_prove_a_carrier_conversion_preserves_zero() {
    let module = Fixture::new().function(&[Type::I64, Type::I1], &[Type::I1], |mut body| {
        let input = body.parameter::<I64>(0)?;
        let enabled = body.parameter::<I1>(1)?;
        let high = input
            .unsigned()
            .shr(enabled.select(32, 0))
            .truncate::<I32>();
        body.if_(enabled, |arm| arm.return_(high.ne(0)))?;
        body.return_(false)
    });
    assert_eq!(wraps(&module), 0);
    assert_eq!(
        module
            .instantiate()
            .call::<i32>((0x1_0000_0000_i64, 1))
            .unwrap(),
        1
    );
    assert_eq!(module.instantiate().call::<i32>((255_i64, 1)).unwrap(), 0);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn zero_preserving_conversions_execute_in_v8() {
    let module = preserves_zero();
    for (input, expected) in [(255, [0, 1, 1]), (-1, [1, 0, 1])] {
        assert_eq!(
            module.run_v8(&Input::call("run", &[Value::I64(input)])),
            Observation::returned(&expected.map(Value::I32))
        );
    }
}
