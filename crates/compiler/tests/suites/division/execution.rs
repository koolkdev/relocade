use super::{input, operators, Kind, KINDS};
use crate::fixture::Fixture;
use crate::wasm::{Input, MemoryBytes, Observation, TestModule, Value};
use wasm86_compiler::{AtLeast, IntType, Type, Val, I1, I16, I32, I64, I8};
use wasmparser::Operator;

fn narrow<T: IntType>() -> TestModule
where
    I32: AtLeast<T>,
{
    Fixture::new().function(&[Type::I32; 2], &[Type::I32; 6], |body| {
        let left = body.parameter::<I32>(0)?.truncate::<T>().sub(1);
        let right = body.parameter::<I32>(1)?.truncate::<T>().add(1);
        let quotient = left.signed().div(&right);
        let remainder = left.signed().rem(&right);
        let unsigned = left.unsigned().div(&right);
        let unsigned_remainder = left.unsigned().rem(right);
        body.return_((
            quotient.unsigned().extend::<I32>(),
            quotient.signed().extend::<I32>(),
            remainder.unsigned().extend::<I32>(),
            remainder.signed().extend::<I32>(),
            unsigned.unsigned().extend::<I32>(),
            unsigned_remainder.unsigned().extend::<I32>(),
        ))
    })
}

#[test]
fn dirty_narrow_operands_and_signed_results_observe_only_their_logical_bits() {
    for (module, arguments, expected) in [
        (
            narrow::<I1>(),
            [0x1234_0002, 0x1234_0000],
            [1, -1, 0, 0, 1, 0],
        ),
        (
            narrow::<I8>(),
            [0x1234_00fa, 0x1234_0002],
            [254, -2, 255, -1, 83, 0],
        ),
        (
            narrow::<I16>(),
            [0x1234_fffa, 0x1234_0002],
            [65534, -2, 65535, -1, 21843, 0],
        ),
    ] {
        assert_eq!(
            module
                .instantiate()
                .call_values("run", &arguments.map(Value::I32))
                .unwrap(),
            expected.map(Value::I32)
        );
    }
}

fn snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[21, 0, 0, 0, 4, 0, 0, 0]);
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let branch = body.parameter::<I1>(0)?;
        let dividend = body.load::<I32>(memory, 0)?;
        let divisor = body.load::<I32>(memory, 4)?;
        let quotient = dividend.unsigned().div(divisor);
        body.store::<I32>(memory, 0, 99)?;
        body.store::<I32>(memory, 4, 9)?;
        let result = body.if_value::<I32>(
            branch,
            |arm| arm.yield_(quotient.add(&quotient)),
            |arm| arm.yield_(quotient.sub(1)),
        )?;
        body.return_(result)
    })
}

#[test]
fn division_consumers_keep_original_operand_snapshots_across_branch_writes() {
    let module = snapshot();
    for (branch, expected) in [(0, 4), (1, 10)] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(branch).unwrap(), expected);
        assert_eq!(&instance.memory("state")[..8], &[99, 0, 0, 0, 9, 0, 0, 0]);
    }
}

#[test]
fn repeated_division_and_remainder_calculations_share_their_body_expression() {
    for kind in KINDS {
        let module = Fixture::new().function(&[Type::I32; 2], &[Type::I32], |body| {
            let left = body.parameter::<I32>(0)?;
            let right = body.parameter::<I32>(1)?;
            let first = kind.apply(&left, &right);
            let second = kind.apply(left, right);
            assert!(first.same_expression(&second));
            body.return_(first.add(second))
        });
        assert_eq!(
            operators(module.bytes())
                .iter()
                .filter(|op| matches!(
                    op,
                    Operator::I32DivU | Operator::I32DivS | Operator::I32RemU | Operator::I32RemS
                ))
                .count(),
            1
        );
        assert_eq!(
            module.instantiate().call::<i32>((17, 3)).unwrap(),
            if matches!(kind, Kind::DivUnsigned | Kind::DivSigned) {
                10
            } else {
                4
            }
        );
    }
}

fn signed_remainder<T: IntType>() -> TestModule {
    Fixture::new().expression(&[T::TYPE; 2], |body| {
        body.parameter::<T>(0)
            .unwrap()
            .signed()
            .rem(body.parameter::<T>(1).unwrap())
    })
}

#[test]
fn signed_minimum_remainder_over_minus_one_is_zero_at_both_native_widths() {
    for (module, arguments) in [
        (
            signed_remainder::<I32>(),
            [Value::I32(i32::MIN), Value::I32(-1)],
        ),
        (
            signed_remainder::<I64>(),
            [Value::I64(i64::MIN), Value::I64(-1)],
        ),
    ] {
        let expected = if matches!(arguments[0], Value::I64(_)) {
            Value::I64(0)
        } else {
            Value::I32(0)
        };
        assert_eq!(
            module.instantiate().call_values("run", &arguments).unwrap(),
            [expected]
        );
    }
    for (value, expected) in [
        (
            Val::<I32>::from(i32::MIN)
                .signed()
                .rem(-1)
                .unsigned()
                .extend::<I64>(),
            0_i64,
        ),
        (
            Val::<I64>::from(0x8000_0000_0000_0000_u64).signed().rem(-1),
            0,
        ),
    ] {
        let module = Fixture::new().expression(&[], |_| value);
        assert_eq!(module.instantiate().call::<i64>(()).unwrap(), expected);
        assert!(!operators(module.bytes())
            .iter()
            .any(|op| matches!(op, Operator::I32RemS | Operator::I64RemS)));
    }
}

fn checked_signed<T: IntType>(minimum: Val<T>) -> TestModule {
    Fixture::new().function(&[T::TYPE; 2], &[Type::I1, T::TYPE, T::TYPE], |mut body| {
        let left = body.parameter::<T>(0)?;
        let right = body.parameter::<T>(1)?;
        let invalid = right.eq(0).or(left.eq(&minimum).and(right.eq(-1)));
        body.if_(invalid, |arm| {
            arm.return_((true, Val::<T>::from(0), Val::<T>::from(0)))
        })?;
        let quotient = left.signed().div(&right);
        body.if_(
            quotient.signed().lt(-128).or(quotient.signed().ge(128)),
            |arm| arm.return_((true, Val::<T>::from(0), Val::<T>::from(0))),
        )?;
        body.return_((false, quotient, left.signed().rem(right)))
    })
}

// Caller checks return an ordinary status before a quotient is used outside its domain.
const CHECKED_CASES: &[(i32, i32, bool, i32, i32)] = &[
    (7, 0, true, 0, 0),
    (128, 1, true, 0, 0),
    (-129, 1, true, 0, 0),
    (-128, 1, false, -128, 0),
    (-7, 3, false, -2, -1),
    (7, -3, false, -2, 1),
];

#[test]
fn caller_checks_precede_shared_quotient_consumers_and_remainder() {
    fn check<T: IntType>(minimum: Val<T>, minimum_bits: u64) {
        let module = checked_signed(minimum);
        let mut instance = module.instantiate();
        for &(left, right, rejected, quotient, remainder) in CHECKED_CASES {
            assert_eq!(
                instance
                    .call_values(
                        "run",
                        &[
                            input::<T>(left as i64 as u64),
                            input::<T>(right as i64 as u64)
                        ]
                    )
                    .unwrap(),
                [
                    Value::I32(i32::from(rejected)),
                    input::<T>(quotient as i64 as u64),
                    input::<T>(remainder as i64 as u64)
                ]
            );
        }
        assert_eq!(
            instance
                .call_values("run", &[input::<T>(minimum_bits), input::<T>(u64::MAX)])
                .unwrap(),
            [Value::I32(1), input::<T>(0), input::<T>(0)]
        );
    }
    check::<I32>(i32::MIN.into(), 0x8000_0000);
    check::<I64>(0x8000_0000_0000_0000_u64.into(), 0x8000_0000_0000_0000);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn caller_checks_narrow_observations_and_operand_snapshots_execute_in_v8() {
    let module = checked_signed::<I32>(i32::MIN.into());
    for &(left, right, rejected, quotient, remainder) in CHECKED_CASES {
        assert_eq!(
            module.run_v8(&Input::call("run", &[Value::I32(left), Value::I32(right)])),
            Observation::returned(&[
                Value::I32(i32::from(rejected)),
                Value::I32(quotient),
                Value::I32(remainder)
            ])
        );
    }
    assert_eq!(
        checked_signed::<I64>(0x8000_0000_0000_0000_u64.into())
            .run_v8(&Input::call("run", &[Value::I64(i64::MIN), Value::I64(-1)])),
        Observation::returned(&[Value::I32(1), Value::I64(0), Value::I64(0)])
    );
    assert_eq!(
        signed_remainder::<I64>()
            .run_v8(&Input::call("run", &[Value::I64(i64::MIN), Value::I64(-1)])),
        Observation::returned(&[Value::I64(0)])
    );
    for (module, arguments, expected) in [
        (
            narrow::<I1>(),
            [0x1234_0002, 0x1234_0000],
            [1, -1, 0, 0, 1, 0],
        ),
        (
            narrow::<I8>(),
            [0x1234_00fa, 0x1234_0002],
            [254, -2, 255, -1, 83, 0],
        ),
        (
            narrow::<I16>(),
            [0x1234_fffa, 0x1234_0002],
            [65534, -2, 65535, -1, 21843, 0],
        ),
    ] {
        assert_eq!(
            module.run_v8(&Input::call("run", &arguments.map(Value::I32))),
            Observation::returned(&expected.map(Value::I32))
        );
    }
    assert_eq!(
        snapshot().run_v8(
            &Input::call("run", &[Value::I32(1)])
                .with_memories(&[MemoryBytes::new("state", &[21, 0, 0, 0, 4, 0, 0, 0])])
        ),
        Observation::returned(&[Value::I32(10)])
            .with_memories(&[MemoryBytes::new("state", &[99, 0, 0, 0, 9, 0, 0, 0])])
    );
}
