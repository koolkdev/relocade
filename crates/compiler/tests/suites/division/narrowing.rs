use super::operators;
use crate::fixture::Fixture;
use crate::wasm::{Input, Observation, TestModule, Value};
use wasm86_compiler::{Type, I1, I32, I64};
use wasmparser::Operator;

struct Case {
    dividend: u32,
    divisor: u32,
    quotient: i64,
    remainder: i64,
}

const CASES: &[Case] = &[
    Case {
        dividend: u32::MAX,
        divisor: 1,
        quotient: 4_294_967_295,
        remainder: 0,
    },
    Case {
        dividend: u32::MAX,
        divisor: 0x8000_0000,
        quotient: 1,
        remainder: 2_147_483_647,
    },
    Case {
        dividend: 0x8000_0000,
        divisor: u32::MAX,
        quotient: 0,
        remainder: 2_147_483_648,
    },
    Case {
        dividend: 0xffff_fffe,
        divisor: 3,
        quotient: 1_431_655_764,
        remainder: 2,
    },
    Case {
        dividend: 0,
        divisor: 17,
        quotient: 0,
        remainder: 0,
    },
];

fn bounded_division() -> TestModule {
    Fixture::new().function(&[Type::I32; 2], &[Type::I64; 3], |body| {
        let dividend = body.parameter::<I32>(0)?.unsigned().extend::<I64>();
        let divisor = body.parameter::<I32>(1)?.unsigned().extend::<I64>();
        let quotient = dividend.unsigned().div(&divisor);
        let remainder = dividend.unsigned().rem(divisor);
        body.return_((&quotient, remainder, quotient.add(0x1_0000_0000_u64)))
    })
}

fn assert_unsigned_width(module: &TestModule, bits: u8) {
    let widths: Vec<_> = operators(module.bytes())
        .iter()
        .filter_map(|operator| match operator {
            Operator::I32DivU | Operator::I32RemU => Some(32),
            Operator::I64DivU | Operator::I64RemU => Some(64),
            _ => None,
        })
        .collect();
    assert_eq!(widths, [bits; 2]);
}

#[test]
fn bounded_unsigned_operands_use_i32_and_keep_i64_results() {
    let module = bounded_division();
    assert_unsigned_width(&module, 32);
    let mut instance = module.instantiate();
    for case in CASES {
        assert_eq!(
            instance.call::<(i64, i64, i64)>((case.dividend as i32, case.divisor as i32)),
            Ok((case.quotient, case.remainder, case.quotient + 0x1_0000_0000))
        );
    }
}

#[test]
fn bounds_from_masked_i64_calculations_also_allow_narrowing() {
    let module = Fixture::new().function(&[Type::I64; 2], &[Type::I64; 2], |body| {
        let dividend = body.parameter::<I64>(0)?.and(u32::MAX);
        let divisor = body.parameter::<I64>(1)?.and(u32::MAX);
        body.return_((
            dividend.unsigned().div(&divisor),
            dividend.unsigned().rem(divisor),
        ))
    });
    assert_unsigned_width(&module, 32);
    assert_eq!(
        module
            .instantiate()
            .call::<(i64, i64)>((0x1234_ffff_ffff_i64, 0x5678_8000_0000_i64)),
        Ok((1, 2_147_483_647))
    );
}

#[test]
fn either_unbounded_operand_keeps_unsigned_division_wide() {
    for bounded_dividend in [false, true] {
        let module = Fixture::new().function(&[Type::I64; 2], &[Type::I64; 2], |body| {
            let mut dividend = body.parameter::<I64>(0)?;
            let mut divisor = body.parameter::<I64>(1)?;
            if bounded_dividend {
                dividend = dividend.and(u32::MAX);
            } else {
                divisor = divisor.and(u32::MAX);
            }
            body.return_((
                dividend.unsigned().div(&divisor),
                dividend.unsigned().rem(divisor),
            ))
        });
        assert_unsigned_width(&module, 64);
        let (arguments, expected) = if bounded_dividend {
            ((0xffff_ffff_i64, 0x1_0000_0000_i64), (0, 4_294_967_295))
        } else {
            ((0x1_0000_0005_i64, 3_i64), (1_431_655_767, 0))
        };
        assert_eq!(
            module.instantiate().call::<(i64, i64)>(arguments),
            Ok(expected)
        );
    }
}

#[test]
fn unsigned_width_bounds_do_not_change_signed_division() {
    let module = Fixture::new().function(&[Type::I32; 2], &[Type::I64; 2], |body| {
        let dividend = body.parameter::<I32>(0)?.unsigned().extend::<I64>();
        let divisor = body.parameter::<I32>(1)?.unsigned().extend::<I64>();
        body.return_((
            dividend.signed().div(&divisor),
            dividend.signed().rem(divisor),
        ))
    });
    let ops = operators(module.bytes());
    assert!(ops.iter().any(|op| matches!(op, Operator::I64DivS)));
    assert!(ops.iter().any(|op| matches!(op, Operator::I64RemS)));
    assert!(!ops
        .iter()
        .any(|op| matches!(op, Operator::I32DivS | Operator::I32RemS)));
    assert_eq!(
        module.instantiate().call::<(i64, i64)>((-1, i32::MIN)),
        Ok((1, 2_147_483_647))
    );
}

fn guarded_division() -> TestModule {
    Fixture::new().function(
        &[Type::I1, Type::I64, Type::I32, Type::I32],
        &[Type::I64; 2],
        |mut body| {
            let stop = body.parameter::<I1>(0)?;
            let wide = body.parameter::<I64>(1)?;
            let low_dividend = body.parameter::<I32>(2)?.unsigned().extend::<I64>();
            let low_divisor = body.parameter::<I32>(3)?.unsigned().extend::<I64>();
            let dividend = stop.select(&wide, low_dividend);
            let divisor = stop.select(wide, low_divisor);
            let quotient = dividend.unsigned().div(&divisor);
            let remainder = dividend.unsigned().rem(divisor);
            body.if_(stop, |arm| arm.return_((7_u64, 9_u64)))?;
            body.return_((quotient, remainder))
        },
    )
}

#[test]
fn a_returning_guard_exposes_operand_bounds_to_folding() {
    let module = guarded_division();
    assert_unsigned_width(&module, 32);
    for (stop, expected) in [(0, (1, 2_147_483_647)), (1, (7, 9))] {
        assert_eq!(
            module
                .instantiate()
                .call::<(i64, i64)>((stop, 0x1_0000_0000_i64, -1, i32::MIN)),
            Ok(expected)
        );
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn narrowed_results_and_guarded_bounds_execute_in_v8() {
    let module = bounded_division();
    for case in CASES {
        assert_eq!(
            module.run_v8(&Input::call(
                "run",
                &[
                    Value::I32(case.dividend as i32),
                    Value::I32(case.divisor as i32)
                ]
            )),
            Observation::returned(&[
                Value::I64(case.quotient),
                Value::I64(case.remainder),
                Value::I64(case.quotient + 0x1_0000_0000)
            ])
        );
    }
    let module = guarded_division();
    for (stop, expected) in [(0, [1, 2_147_483_647]), (1, [7, 9])] {
        assert_eq!(
            module.run_v8(&Input::call(
                "run",
                &[
                    Value::I32(stop),
                    Value::I64(0x1_0000_0000),
                    Value::I32(-1),
                    Value::I32(i32::MIN)
                ]
            )),
            Observation::returned(&expected.map(Value::I64))
        );
    }
}
