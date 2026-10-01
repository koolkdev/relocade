use super::*;
use wasm86_compiler::BuildError;

fn product(signed: bool, order: &[usize]) -> TestModule {
    Fixture::new().function(&[Type::I64; 2], &vec![Type::I64; order.len()], |body| {
        let left = body.parameter::<I64>(0)?;
        let right = body.parameter::<I64>(1)?;
        let (low, high) = if signed {
            left.signed().mul_wide(right)
        } else {
            left.unsigned().mul_wide(right)
        };
        let halves = [low, high];
        body.return_(
            order
                .iter()
                .map(|&index| halves[index].clone())
                .collect::<Vec<_>>(),
        )
    })
}

fn expected(left: u64, right: u64, signed: bool) -> [i64; 2] {
    let product = if signed {
        ((left as i64 as i128) * (right as i64 as i128)) as u128
    } else {
        u128::from(left) * u128::from(right)
    };
    [product as i64, (product >> 64) as i64]
}

fn multiplications(module: &TestModule) -> usize {
    operators(module.bytes())
        .iter()
        .filter(|op| matches!(op, Operator::I64Mul))
        .count()
}

#[test]
fn wide_products_preserve_both_halves_at_limb_and_sign_boundaries() {
    let boundaries = [
        0,
        1,
        2,
        0xffff_ffff,
        0x1_0000_0000,
        0x1_ffff_ffff,
        0x7fff_ffff_ffff_ffff,
        0x8000_0000_0000_0000,
        0x8000_0000_0000_0001,
        u64::MAX,
    ];
    for signed in [false, true] {
        let module = product(signed, &[0, 1]);
        let mut instance = module.instantiate();
        let mut check = |left, right| {
            let [low, high] = expected(left, right, signed);
            assert_eq!(
                instance.call::<(i64, i64)>((left as i64, right as i64)),
                Ok((low, high)),
                "{left:#x} * {right:#x}, signed={signed}"
            );
        };
        for left in boundaries {
            for right in boundaries {
                check(left, right);
            }
        }
        let mut bits = 0x1234_5678_9abc_def0_u64;
        for _ in 0..1024 {
            bits ^= bits << 13;
            bits ^= bits >> 7;
            bits ^= bits << 17;
            check(bits, bits.rotate_left(29).wrapping_add(0xdead_beef));
        }
    }
}

#[test]
fn wide_result_order_and_independent_demand_keep_one_producer() {
    for order in [&[0, 1][..], &[1, 0], &[0], &[1], &[1, 0, 1, 0], &[]] {
        let module = product(false, order);
        // The portable product uses four limb multiplications, shared by all uses.
        assert_eq!(
            multiplications(&module),
            if order.is_empty() { 0 } else { 4 }
        );
        let halves = [-2, 1]; // (2^64 - 1) * 2
        assert_eq!(
            module
                .instantiate()
                .call_values("run", &[Value::I64(-1), Value::I64(2)])
                .unwrap(),
            order
                .iter()
                .map(|&index| Value::I64(halves[index]))
                .collect::<Vec<_>>()
        );
    }
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
fn high_product_demand_retains_its_read_snapshot() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0xff; 8]);
    let module = fixture.function(&[], &[Type::I64], |mut body| {
        let before = body.load::<I64>(state, 0)?;
        let (_, high) = before.unsigned().mul_wide(2);
        body.store::<I64>(state, 0, 0)?;
        body.return_(high)
    });
    assert_eq!(module.instantiate().call::<i64>(()), Ok(1));
}

#[test]
fn wide_components_share_across_dominated_and_sibling_uses() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0; 8]);
    let module = fixture.function(
        &[Type::I1, Type::I64, Type::I64],
        &[Type::I64],
        |mut body| {
            let condition = body.parameter::<I1>(0)?;
            let left = body.parameter::<I64>(1)?;
            let right = body.parameter::<I64>(2)?;
            let (low, high) = left.unsigned().mul_wide(&right);
            let (_, repeated_high) = left.unsigned().mul_wide(right);
            assert!(high.same_expression(&repeated_high));
            assert!(!low.same_expression(&high));
            body.if_else(
                condition,
                |mut arm| arm.store(state, 0, low),
                |mut arm| arm.store(state, 0, &high),
            )?;
            body.return_(repeated_high)
        },
    );
    assert_eq!(multiplications(&module), 4);
    for (condition, stored) in [(0, 1_i64), (1, -2_i64)] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i64>((condition, -1_i64, 2_i64)), Ok(1));
        assert_eq!(&instance.memory("state")[..8], &stored.to_le_bytes());
    }
}

#[test]
fn separately_placed_wide_components_do_not_escape_their_branch() {
    let module = Fixture::new().function(
        &[Type::I1, Type::I64, Type::I64],
        &[Type::I64],
        |mut body| {
            let condition = body.parameter::<I1>(0)?;
            let (low, high) = body
                .parameter::<I64>(1)?
                .unsigned()
                .mul_wide(body.parameter::<I64>(2)?);
            let result =
                body.if_value::<I64>(condition, |arm| arm.yield_(high), |arm| arm.yield_(low))?;
            body.return_(result)
        },
    );
    for (condition, expected) in [(0, -2), (1, 1)] {
        assert_eq!(
            module.instantiate().call::<i64>((condition, -1_i64, 2_i64)),
            Ok(expected)
        );
    }
}

#[test]
fn wide_results_specialize_independently_under_operand_facts() {
    let module = Fixture::new().function(&[Type::I64], &[Type::I64; 2], |mut body| {
        let input = body.parameter::<I64>(0)?;
        let (low, high) = input.unsigned().mul_wide(u64::MAX);
        let result = body.if_value::<(I64, I64)>(
            input.eq(u64::MAX),
            |arm| arm.yield_((&high, &low)),
            |arm| arm.yield_((0, 0)),
        )?;
        body.return_(result)
    });
    assert_eq!(multiplications(&module), 0);
    assert_eq!(module.instantiate().call::<(i64, i64)>(-1_i64), Ok((-2, 1)));
    assert_eq!(module.instantiate().call::<(i64, i64)>(7_i64), Ok((0, 0)));
}

fn iterated_products() -> TestModule {
    Fixture::new().function(&[Type::I32, Type::I64], &[Type::I64; 2], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let seed = body.parameter::<I64>(1)?;
        let pair = body.loop_::<(I32, I64, I64), (I64, I64)>(
            (count, seed, 0),
            |mut iteration, labels, (left, low, high)| {
                iteration.if_(left.eq(0), |done| done.branch(&labels.exit, (&low, &high)))?;
                let (next_low, next_high) = low.unsigned().mul_wide(u64::MAX);
                iteration.branch(&labels.again, (left.sub(1), next_low, high.add(next_high)))
            },
        )?;
        body.return_(pair)
    })
}

#[test]
fn wide_lowering_temporaries_preserve_loop_carried_values() {
    let module = iterated_products();
    assert_eq!(
        module.instantiate().call::<(i64, i64)>((0, 3_i64)),
        Ok((3, 0))
    );
    assert_eq!(
        module.instantiate().call::<(i64, i64)>((1, 3_i64)),
        Ok((-3, 2))
    );
    assert_eq!(
        module.instantiate().call::<(i64, i64)>((2, 3_i64)),
        Ok((3, -2))
    );
    assert_eq!(
        module.instantiate().call::<(i64, i64)>((3, 3_i64)),
        Ok((-3, 0))
    );
}

#[test]
fn every_wide_result_retains_operand_visibility_after_folding() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0; 8]);
    let _module = fixture.function(&[Type::I1], &[], |mut body| {
        let mut escaped = None;
        body.if_(body.parameter::<I1>(0)?, |mut arm| {
            let zero = arm.load::<I64>(state, 0)?.and(0);
            escaped = Some(zero.unsigned().mul_wide(7));
            Ok(())
        })?;
        let (low, high) = escaped.unwrap();
        for value in [low, high] {
            assert_eq!(body.value(value).err(), Some(BuildError::OutOfScope));
        }
        body.return_(())
    });
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn wide_products_and_loop_temporaries_execute_in_v8() {
    for signed in [false, true] {
        for order in [&[0, 1][..], &[1, 0], &[1]] {
            let module = product(signed, order);
            for (left, right) in [
                (u64::MAX, u64::MAX),
                (1 << 63, 2),
                (0xffff_ffff, 0x1_ffff_ffff),
            ] {
                let halves = expected(left, right, signed);
                assert_eq!(
                    module.run_v8(&Input::call(
                        "run",
                        &[Value::I64(left as i64), Value::I64(right as i64)]
                    )),
                    Observation::returned(
                        &order
                            .iter()
                            .map(|&index| Value::I64(halves[index]))
                            .collect::<Vec<_>>()
                    )
                );
            }
        }
    }
    assert_eq!(
        iterated_products().run_v8(&Input::call("run", &[Value::I32(3), Value::I64(3)])),
        Observation::returned(&[Value::I64(-3), Value::I64(0)])
    );
}
