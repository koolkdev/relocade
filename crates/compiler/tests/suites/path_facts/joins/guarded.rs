//! Reuse across repeated guards preserves skipped paths and iteration inputs.
use super::*;
use wasm86_compiler::I64;

fn guarded_divisions(independent: bool) -> TestModule {
    Fixture::new().function(
        &[Type::I1, Type::I1, Type::I32, Type::I32],
        &[Type::I32],
        |mut body| {
            let first = body.parameter::<I1>(0)?;
            let second = body.parameter::<I1>(1)?;
            let numerator = body.parameter::<I32>(2)?;
            let denominator = body.parameter::<I32>(3)?;
            let quotient = numerator.unsigned().div(denominator);
            let first_quotient =
                body.if_value::<I32>(&first, |arm| arm.yield_(&quotient), |arm| arm.yield_(0))?;
            let repeated_sum = body.if_value::<I32>(
                &first,
                |arm| arm.yield_(first_quotient.add(&quotient)),
                |arm| arm.yield_(0),
            )?;
            if independent {
                let total = body.if_value::<I32>(
                    &second,
                    |arm| arm.yield_(repeated_sum.add(&quotient)),
                    |arm| arm.yield_(0),
                )?;
                body.return_(total)
            } else {
                body.return_(repeated_sum)
            }
        },
    )
}

#[test]
fn repeated_guards_reuse_division_without_executing_the_skipped_arm() {
    let module = guarded_divisions(false);
    assert_eq!(count(&module, |op| matches!(op, Operator::I32DivU)), 1);
    for (first, denominator, expected) in [(0, 0, 0), (0, 3, 0), (1, 3, 8)] {
        assert_eq!(
            module
                .instantiate()
                .call::<i32>((first, 0, 12, denominator)),
            Ok(expected)
        );
    }
}

#[test]
fn an_independent_guard_cannot_reuse_an_uncomputed_value() {
    let module = guarded_divisions(true);
    for (first, second, denominator, expected) in
        [(0, 0, 0, 0), (0, 1, 3, 4), (1, 0, 3, 0), (1, 1, 3, 12)]
    {
        assert_eq!(
            module
                .instantiate()
                .call::<i32>((first, second, 12, denominator)),
            Ok(expected)
        );
    }
}

fn masked_guards() -> TestModule {
    Fixture::new().function(
        &[Type::I32, Type::I32, Type::I32],
        &[Type::I32],
        |mut body| {
            let input = body.parameter::<I32>(0)?;
            let numerator = body.parameter::<I32>(1)?;
            let denominator = body.parameter::<I32>(2)?;
            let quotient = numerator.unsigned().div(denominator);
            let first = body.if_value::<I32>(
                input.eq(0),
                |arm| arm.yield_(&quotient),
                |arm| arm.yield_(0),
            )?;
            let second = body.if_value::<I32>(
                input.truncate::<I8>().eq(0),
                |arm| arm.yield_(first.add(&quotient)),
                |arm| arm.yield_(0),
            )?;
            body.return_(second)
        },
    )
}

#[test]
fn a_low_byte_guard_does_not_prove_a_whole_word_guard() {
    let module = masked_guards();
    for (input, denominator, expected) in [(0, 3, 8), (0x100, 3, 4), (1, 0, 0)] {
        assert_eq!(
            module.instantiate().call::<i32>((input, 12, denominator)),
            Ok(expected)
        );
    }
}

fn guarded_loop() -> TestModule {
    Fixture::new().function(&[Type::I32], &[Type::I32], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let result =
            body.loop_::<(I32, I32), I32>((count, 0), |mut iteration, labels, (left, total)| {
                let enabled = left.and(1).ne(0);
                let quotient = left.unsigned().div(3);
                let first = iteration.if_value::<I32>(
                    &enabled,
                    |arm| arm.yield_(&quotient),
                    |arm| arm.yield_(0),
                )?;
                let second = iteration.if_value::<I32>(
                    &enabled,
                    |arm| arm.yield_(first.add(&quotient)),
                    |arm| arm.yield_(0),
                )?;
                let total = total.add(second);
                iteration.branch_if(left.eq(0), &labels.exit, &total)?;
                iteration.branch(&labels.again, (left.sub(1), total))
            })?;
        body.return_(result)
    })
}

#[test]
fn guarded_reuse_observes_current_loop_operands() {
    let module = guarded_loop();
    for (input, expected) in [(0, 0), (1, 0), (3, 2), (4, 2), (9, 14), (10, 14)] {
        assert_eq!(module.instantiate().call::<i32>((input,)), Ok(expected));
    }
}

#[test]
#[ignore = "requires Node.js with V8"]
fn v8_guarded_values_preserve_conditions_and_loop_iterations() {
    for (module, cases) in [
        (
            guarded_divisions(false),
            vec![(vec![0, 0, 12, 0], 0), (vec![1, 0, 12, 3], 8)],
        ),
        (
            guarded_divisions(true),
            vec![
                (vec![0, 0, 12, 0], 0),
                (vec![0, 1, 12, 3], 4),
                (vec![1, 1, 12, 3], 12),
            ],
        ),
        (
            masked_guards(),
            vec![(vec![0x100, 12, 3], 4), (vec![1, 12, 0], 0)],
        ),
        (guarded_loop(), vec![(vec![0], 0), (vec![9], 14)]),
    ] {
        for (arguments, expected) in cases {
            assert_eq!(
                module.run_v8(&Input::call(
                    "run",
                    &arguments.into_iter().map(Value::I32).collect::<Vec<_>>()
                )),
                Observation::returned(&[Value::I32(expected)])
            );
        }
    }
    check_converging_recipes(true);
    check_rewritten_predicate(true);
}

fn converging_recipes() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 12]);
    fixture.function(
        &[Type::I1, Type::I1, Type::I32, Type::I32],
        &[Type::I32],
        |mut body| {
            let active = body.parameter::<I1>(0)?;
            let other = body.parameter::<I1>(1)?;
            let input = body.parameter::<I32>(2)?;
            let factor = body.parameter::<I32>(3)?;
            let first = active.select(&input, input.add(5)).mul(&factor);
            let second = active.select(&input, input.add(9)).mul(&factor);
            body.if_(&active, |mut arm| arm.store(memory, 0, first))?;
            body.if_(&active, |mut arm| arm.store(memory, 4, &second))?;
            body.if_(&other, |mut arm| arm.store(memory, 8, second))?;
            body.return_(17)
        },
    )
}

fn check_converging_recipes(v8: bool) {
    let module = converging_recipes();
    assert_eq!(count(&module, |op| matches!(op, Operator::I32Mul)), 2);
    for (active, other, expected) in [
        (0, 0, [0_i32, 0, 0]),
        (0, 1, [0, 0, 42]),
        (1, 0, [15, 15, 0]),
        (1, 1, [15, 15, 15]),
    ] {
        let expected: Vec<_> = expected.into_iter().flat_map(i32::to_le_bytes).collect();
        if v8 {
            assert_eq!(
                module.run_v8(
                    &Input::call("run", &[active, other, 5, 3].map(Value::I32))
                        .with_memories(&[MemoryBytes::new("state", &[0; 12])])
                ),
                Observation::returned(&[Value::I32(17)])
                    .with_memories(&[MemoryBytes::new("state", &expected)])
            );
        } else {
            let mut instance = module.instantiate();
            assert_eq!(instance.call::<i32>((active, other, 5, 3)), Ok(17));
            assert_eq!(&instance.memory("state")[..12], expected);
        }
    }
}

#[test]
fn distinct_recipes_share_the_residual_proved_by_their_guards() {
    check_converging_recipes(false);
}

fn check_rewritten_predicate(v8: bool) {
    let initial: Vec<_> = [17_i32, 19]
        .into_iter()
        .flat_map(i32::to_le_bytes)
        .collect();
    for through_conversion in [false, true] {
        let mut fixture = Fixture::new();
        let memory = fixture.memory("state", &initial);
        let module = fixture.function(
            &[Type::I1, Type::I1, Type::I32],
            &[Type::I32],
            |mut body| {
                let active = body.parameter::<I1>(0)?;
                let invalid = body.parameter::<I1>(1)?;
                let input = body.parameter::<I32>(2)?;
                let masked_input = input.and(0x7fff);
                let conditional_zero = if through_conversion {
                    invalid
                        .select(0xffff_u64, masked_input.unsigned().extend::<I64>())
                        .eq(0_u64)
                } else {
                    invalid.select(0xffff, &input).and(0x7fff).eq(0)
                };
                // Both paths observe the masked input before a join can retain
                // its zero test. The later guard proves that test false.
                body.store(memory, 4, &masked_input)?;
                body.if_(&active, |mut arm| {
                    arm.store(memory, 0, masked_input.eq(0).unsigned().extend::<I32>())
                })?;
                body.if_(
                    active.and(invalid.eq(false)).and(masked_input.ne(0)),
                    |arm| arm.return_(conditional_zero.select(input.clz(), 11)),
                )?;
                body.return_(13)
            },
        );
        assert_eq!(count(&module, |op| matches!(op, Operator::I32Clz)), 0);
        for (arguments, result, stores) in [
            ([0, 0, 5], 13, [17_i32, 5]),
            ([1, 0, 0], 13, [1, 0]),
            ([1, 0, 5], 11, [0, 5]),
            ([1, 1, 5], 13, [0, 5]),
            ([1, 0, 0x8000], 13, [1, 0]),
            ([1, 0, -1], 11, [0, 0x7fff]),
        ] {
            let expected: Vec<_> = stores.into_iter().flat_map(i32::to_le_bytes).collect();
            if v8 {
                assert_eq!(
                    module.run_v8(
                        &Input::call("run", &arguments.map(Value::I32))
                            .with_memories(&[MemoryBytes::new("state", &initial)])
                    ),
                    Observation::returned(&[Value::I32(result)])
                        .with_memories(&[MemoryBytes::new("state", &expected)])
                );
            } else {
                let [active, invalid, input] = arguments;
                let mut instance = module.instantiate();
                assert_eq!(instance.call::<i32>((active, invalid, input)), Ok(result));
                assert_eq!(&instance.memory("state")[..8], expected);
            }
        }
    }
}

#[test]
fn rewritten_predicates_fold_before_joined_value_reuse() {
    check_rewritten_predicate(false);
}
