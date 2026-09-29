use crate::fixture::Fixture;
use crate::wasm::TestModule;
use wasm86_compiler::{Type, I1, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload};

fn count(bytes: &[u8], predicate: impl Fn(&Operator<'_>) -> bool) -> usize {
    let mut count = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                count += usize::from(predicate(&reader.read().unwrap()));
            }
        }
    }
    count
}

fn guarded_join(outward: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 12]);
    fixture.function(
        &[Type::I1, Type::I32, Type::I32],
        &[Type::I32],
        |mut body| {
            let choice = body.parameter::<I1>(0)?;
            let numerator = body.parameter::<I32>(1)?;
            let denominator = body.parameter::<I32>(2)?;
            let quotient = numerator.unsigned().div(&denominator);
            let offset = quotient.and(0xfff);
            if outward {
                body.block::<()>(|mut outer, exit| {
                    outer.if_(&choice, |mut arm| {
                        arm.if_(denominator.eq(0), |fault| fault.return_(17))?;
                        arm.store(memory, 0, &quotient)?;
                        arm.store(memory, 4, &offset)?;
                        arm.branch(&exit, ())
                    })?;
                    outer.if_(denominator.eq(0), |fault| fault.return_(19))?;
                    outer.store(memory, 0, &quotient)?;
                    outer.store(memory, 4, &offset)
                })?;
            } else {
                body.if_else(
                    &choice,
                    |mut arm| {
                        arm.if_(denominator.eq(0), |fault| fault.return_(17))?;
                        arm.store(memory, 0, &quotient)?;
                        arm.store(memory, 4, &offset)
                    },
                    |mut arm| {
                        arm.if_(denominator.eq(0), |fault| fault.return_(19))?;
                        arm.store(memory, 0, &quotient)?;
                        arm.store(memory, 4, &offset)
                    },
                )?;
            }
            body.store(memory, 8, &offset)?;
            body.return_(&quotient)
        },
    )
}

#[test]
fn successful_arm_values_and_their_dependent_masks_reach_the_join() {
    for outward in [false, true] {
        let module = guarded_join(outward);
        assert_eq!(
            count(module.bytes(), |op| matches!(op, Operator::I32DivU)),
            2
        );
        assert_eq!(
            count(module.bytes(), |op| matches!(op, Operator::I32And)),
            2
        );
        for (choice, numerator, denominator, expected, words) in [
            (1, 100, 3, 33, [33u32, 33, 33]),
            (0, 8198, 2, 4099, [4099, 3, 3]),
            (1, 100, 0, 17, [0, 0, 0]),
            (0, 100, 0, 19, [0, 0, 0]),
        ] {
            let mut instance = module.instantiate();
            assert_eq!(
                instance.call::<i32>((choice, numerator, denominator)),
                Ok(expected)
            );
            let bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
            assert_eq!(&instance.memory("state")[..12], bytes);
        }
    }
}

#[test]
fn a_selected_recipe_reuses_the_value_available_on_each_incoming_path() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 4]);
    let module = fixture.function(
        &[Type::I1, Type::I1, Type::I32],
        &[Type::I32],
        |mut body| {
            let early = body.parameter::<I1>(0)?;
            let publish = body.parameter::<I1>(1)?;
            let input = body.parameter::<I32>(2)?;
            let result = publish.select(input.mul(&input), &input);
            body.if_(early, |mut arm| {
                arm.if_(&publish, |mut store| store.store(memory, 0, &result))?;
                arm.return_(&result)
            })?;
            body.if_(&publish, |mut store| store.store(memory, 0, &result))?;
            body.return_(result)
        },
    );
    // Both exits need the recipe. On each exit's publication path the square
    // has already executed; its other incoming path already has the input.
    assert_eq!(
        count(module.bytes(), |op| matches!(op, Operator::I32Mul)),
        2
    );
    for early in [0, 1] {
        for (publish, expected, stored) in [(0, 7, 0_u32), (1, 49, 49)] {
            let mut instance = module.instantiate();
            assert_eq!(instance.call::<i32>((early, publish, 7)), Ok(expected));
            assert_eq!(&instance.memory("state")[..4], &stored.to_le_bytes());
        }
    }
}

#[test]
fn missing_optional_predecessors_keep_zero_divisors_out_of_the_calculation() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 8]);
    let module = fixture.function(&[Type::I32, Type::I32], &[Type::I32], |mut body| {
        let numerator = body.parameter::<I32>(0)?;
        let denominator = body.parameter::<I32>(1)?;
        let quotient = numerator.unsigned().div(&denominator);
        body.if_(denominator.ne(0), |mut arm| arm.store(memory, 0, &quotient))?;
        body.if_(denominator.ne(0), |mut arm| arm.store(memory, 4, &quotient))?;
        body.return_(7)
    });
    let mut zero = module.instantiate();
    assert_eq!(zero.call::<i32>((100, 0)), Ok(7));
    assert_eq!(&zero.memory("state")[..8], &[0; 8]);
    let mut nonzero = module.instantiate();
    assert_eq!(nonzero.call::<i32>((100, 4)), Ok(7));
    assert_eq!(&nonzero.memory("state")[..8], &[25, 0, 0, 0, 25, 0, 0, 0]);
}

#[test]
fn separate_loads_keep_their_snapshots_across_a_write() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[7, 0, 0, 0]);
    let module = fixture.function(&[], &[Type::I32], |mut body| {
        let old = body.load::<I32>(memory, 0)?.add(1);
        body.store::<I32>(memory, 0, 9)?;
        let new = body.load::<I32>(memory, 0)?.add(1);
        body.return_(old.add(&new))
    });
    assert_eq!(module.instantiate().call::<i32>(()), Ok(18));
}

#[test]
fn loop_backedges_preserve_parallel_tuple_swaps_and_current_iteration_values() {
    let module = Fixture::new().function(
        &[Type::I32],
        &[Type::I64, Type::I32, Type::I32],
        |mut body| {
            let count = body.parameter::<I32>(0)?;
            let results = body.loop_::<(I32, I32, I32, I64), (I64, I32, I32)>(
                (count, 3, 7, 100u64),
                |mut iteration, labels, (left, first, second, sum)| {
                    iteration.if_(left.eq(0), |done| {
                        done.branch(&labels.exit, (&sum, &first, &second))
                    })?;
                    iteration.branch(
                        &labels.again,
                        (
                            left.sub(1),
                            &second,
                            &first,
                            sum.add(first.unsigned().extend::<I64>()),
                        ),
                    )
                },
            )?;
            body.return_(results)
        },
    );
    assert_eq!(
        module.instantiate().call::<(i64, i32, i32)>(0),
        Ok((100, 3, 7))
    );
    assert_eq!(
        module.instantiate().call::<(i64, i32, i32)>(3),
        Ok((113, 7, 3))
    );
    assert_eq!(
        module.instantiate().call::<(i64, i32, i32)>(4),
        Ok((120, 3, 7))
    );
}

#[test]
fn conditional_reuse_inside_a_loop_uses_the_current_iteration() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 4]);
    let module = fixture.function(&[Type::I32, Type::I1], &[Type::I32], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let square = body.parameter::<I1>(1)?;
        let result =
            body.loop_::<(I32, I32), I32>((count, 2), |mut iteration, labels, (left, value)| {
                iteration.if_(left.eq(0), |done| done.branch(&labels.exit, &value))?;
                let result = square.select(value.mul(&value), &value);
                iteration.if_(left.eq(1), |mut last| {
                    last.if_(&square, |mut store| store.store(memory, 0, &result))?;
                    last.branch(&labels.exit, &result)
                })?;
                iteration.if_(&square, |mut store| store.store(memory, 0, &result))?;
                iteration.branch(&labels.again, (left.sub(1), result.add(1)))
            })?;
        body.return_(result)
    });
    for (iterations, square, expected, stored) in [
        (0, 1, 2, 0_u32),
        (1, 1, 4, 4),
        (3, 1, 676, 676),
        (3, 0, 4, 0),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((iterations, square)), Ok(expected));
        assert_eq!(&instance.memory("state")[..4], &stored.to_le_bytes());
    }
}

#[test]
fn narrow_signed_calculations_keep_their_carrier_meaning_through_joins() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 2]);
    let module = fixture.function(
        &[Type::I1, Type::I8],
        &[Type::I64, Type::I32],
        |mut body| {
            let choice = body.parameter::<I1>(0)?;
            let byte = body.parameter::<I8>(1)?;
            let shifted = byte.signed().shr(1);
            body.if_else(
                &choice,
                |mut arm| {
                    arm.if_(byte.eq(42), |exit| exit.return_((999u64, 999)))?;
                    arm.store(memory, 0, &shifted)
                },
                |mut arm| arm.store(memory, 1, &shifted),
            )?;
            body.return_((
                shifted.signed().extend::<I64>(),
                shifted.unsigned().extend::<I32>(),
            ))
        },
    );
    for choice in [0, 1] {
        for (byte, expected) in [(255, (-1i64, 255)), (128, (-64, 192)), (127, (63, 63))] {
            assert_eq!(
                module.instantiate().call::<(i64, i32)>((choice, byte)),
                Ok(expected)
            );
        }
    }
    assert_eq!(
        module.instantiate().call::<(i64, i32)>((1, 42)),
        Ok((999, 999))
    );
}
