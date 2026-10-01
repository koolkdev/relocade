//! Exact wide-product folds during construction and path specialization.

use super::*;

fn literal_product(signed: bool, factor: u64, factor_on_left: bool) -> TestModule {
    Fixture::new().function(&[Type::I64], &[Type::I64; 2], |body| {
        let input = body.parameter::<I64>(0)?;
        let (left, right) = if factor_on_left {
            (Val::from(factor), input)
        } else {
            (input, Val::from(factor))
        };
        body.return_(if signed {
            left.signed().mul_wide(right)
        } else {
            left.unsigned().mul_wide(right)
        })
    })
}

fn extended_i32_product(signed: bool) -> TestModule {
    Fixture::new().function(&[Type::I32; 2], &[Type::I64; 3], |body| {
        let left = body.parameter::<I32>(0)?;
        let right = body.parameter::<I32>(1)?;
        let (left, right) = if signed {
            (
                left.signed().extend::<I64>(),
                right.signed().extend::<I64>(),
            )
        } else {
            (
                left.unsigned().extend::<I64>(),
                right.unsigned().extend::<I64>(),
            )
        };
        let (low, high) = if signed {
            left.signed().mul_wide(&right)
        } else {
            left.unsigned().mul_wide(&right)
        };
        // The folded pair and a separately requested scalar share their product.
        body.return_((high, left.mul(right), low))
    })
}

fn guarded_product(signed: bool, factor: u64) -> TestModule {
    Fixture::new().function(&[Type::I64; 2], &[Type::I64; 2], |mut body| {
        let left = body.parameter::<I64>(0)?;
        let right = body.parameter::<I64>(1)?;
        let (low, high) = if signed {
            left.signed().mul_wide(&right)
        } else {
            left.unsigned().mul_wide(&right)
        };
        let result = body.if_value::<(I64, I64)>(
            right.eq(factor),
            |arm| arm.yield_((high, low)),
            |arm| arm.yield_((7, 9)),
        )?;
        body.return_(result)
    })
}

#[test]
fn literal_and_admitted_wide_products_fold_in_result_order() {
    for (signed, left, right, halves) in [
        (false, u64::MAX, u64::MAX, [1, -2]),
        (true, u64::MAX, u64::MAX, [1, 0]),
        (true, 1 << 63, u64::MAX, [i64::MIN, 0]),
        (true, 1 << 63, 2, [0, -1]),
    ] {
        for admitted in [false, true] {
            let module = Fixture::new().function(&[], &[Type::I64; 2], |body| {
                let left = if admitted {
                    body.value::<I64>(left)?
                } else {
                    left.into()
                };
                let pair = if signed {
                    left.signed().mul_wide(right)
                } else {
                    left.unsigned().mul_wide(right)
                };
                body.return_(pair)
            });
            assert!(matches!(operators(module.bytes()).as_slice(),
                [Operator::I64Const { value: low }, Operator::I64Const { value: high }, Operator::Return, Operator::End]
                    if [*low, *high] == halves));
        }
    }
}

#[test]
fn zero_and_one_factors_fold_both_wide_results() {
    for signed in [false, true] {
        for factor in [0, 1] {
            for factor_on_left in [false, true] {
                let module = literal_product(signed, factor, factor_on_left);
                assert_eq!(multiplications(&module), 0);
                let mut instance = module.instantiate();
                for input in [0, 1, i64::MIN, i64::MAX, -1] {
                    let [low, high] = expected(input as u64, factor, signed);
                    assert_eq!(
                        instance.call::<(i64, i64)>(input),
                        Ok((low, high)),
                        "{input} * {factor}, signed={signed}, factor_on_left={factor_on_left}"
                    );
                }
            }
            let module = guarded_product(signed, factor);
            assert_eq!(multiplications(&module), 0);
            let mut instance = module.instantiate();
            for input in [i64::MIN, -1, i64::MAX] {
                let [low, high] = expected(input as u64, factor, signed);
                assert_eq!(
                    instance.call::<(i64, i64)>((input, factor as i64)),
                    Ok((high, low)),
                    "{input} * {factor}, signed={signed}"
                );
                assert_eq!(instance.call::<(i64, i64)>((input, 2_i64)), Ok((7, 9)));
            }
        }
    }
}

#[test]
fn bounded_products_share_one_scalar_multiply() {
    for signed in [false, true] {
        let module = extended_i32_product(signed);
        assert_eq!(multiplications(&module), 1);
        let mut instance = module.instantiate();
        for left in [0, 1, i32::MIN, i32::MAX, -1] {
            for right in [0, 1, i32::MIN, i32::MAX, -1] {
                let extend = |input: i32| {
                    if signed {
                        input as i64 as u64
                    } else {
                        u64::from(input as u32)
                    }
                };
                let [low, high] = expected(extend(left), extend(right), signed);
                assert_eq!(
                    instance.call::<(i64, i64, i64)>((left, right)),
                    Ok((high, low, low)),
                    "{left} * {right}, signed={signed}"
                );
            }
        }
        let module = Fixture::new().function(&[Type::I64; 2], &[Type::I64; 2], |body| {
            let left = body.parameter::<I64>(0)?;
            let factor = body.parameter::<I64>(1)?.and(1);
            body.return_(if signed {
                left.signed().mul_wide(factor)
            } else {
                left.unsigned().mul_wide(factor)
            })
        });
        assert_eq!(multiplications(&module), 1);
        let mut instance = module.instantiate();
        for left in [i64::MIN, i64::MAX, -1, 0, 1] {
            for factor in [0_i64, 1, 2, -1] {
                let [low, high] = expected(left as u64, (factor & 1) as u64, signed);
                assert_eq!(
                    instance.call::<(i64, i64)>((left, factor)),
                    Ok((low, high)),
                    "{left} * ({factor} & 1), signed={signed}"
                );
            }
        }
    }
    let module = Fixture::new().expression(&[Type::I32; 2], |body| {
        let left = body.parameter::<I32>(0).unwrap().unsigned().extend::<I64>();
        let right = body.parameter::<I32>(1).unwrap().unsigned().extend::<I64>();
        left.unsigned().mul_wide(right).1
    });
    assert!(matches!(
        operators(module.bytes()).as_slice(),
        [
            Operator::I64Const { value: 0 },
            Operator::Return,
            Operator::End
        ]
    ));
}

#[test]
fn products_that_exceed_the_proven_width_keep_the_high_half() {
    let signed = literal_product(true, u64::MAX, false);
    assert_eq!(
        signed.instantiate().call::<(i64, i64)>(i64::MIN),
        Ok((i64::MIN, 0))
    );

    let unsigned = Fixture::new().function(&[Type::I64; 2], &[Type::I64; 2], |body| {
        let left = body.parameter::<I64>(0)?.and(0x1_ffff_ffff_u64);
        let right = body.parameter::<I64>(1)?.and(0xffff_ffff_u64);
        body.return_(left.unsigned().mul_wide(right))
    });
    assert_eq!(
        unsigned.instantiate().call::<(i64, i64)>((-1_i64, -1_i64)),
        Ok((-12_884_901_887, 1))
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn folded_wide_products_execute_in_v8() {
    for signed in [false, true] {
        for factor in [0, 1] {
            let [low, high] = expected(i64::MIN as u64, factor, signed);
            assert_eq!(
                literal_product(signed, factor, false)
                    .run_v8(&Input::call("run", &[Value::I64(i64::MIN)])),
                Observation::returned(&[Value::I64(low), Value::I64(high)])
            );
            assert_eq!(
                guarded_product(signed, factor).run_v8(&Input::call(
                    "run",
                    &[Value::I64(i64::MIN), Value::I64(factor as i64)]
                )),
                Observation::returned(&[Value::I64(high), Value::I64(low)])
            );
        }
        let [low, high] = expected(
            if signed {
                i32::MIN as i64 as u64
            } else {
                u64::from(i32::MIN as u32)
            },
            if signed {
                u64::MAX
            } else {
                u64::from(u32::MAX)
            },
            signed,
        );
        assert_eq!(
            extended_i32_product(signed)
                .run_v8(&Input::call("run", &[Value::I32(i32::MIN), Value::I32(-1)])),
            Observation::returned(&[Value::I64(high), Value::I64(low), Value::I64(low)])
        );
    }
}
