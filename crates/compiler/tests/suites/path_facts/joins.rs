//! Values reused after a join remain scoped to paths that pass through it.
use super::*;
use crate::wasm::MemoryBytes;

#[path = "joins/guarded.rs"]
mod guarded;

#[derive(Clone, Copy)]
enum Use {
    Descendant,
    Sibling,
}

fn joined_square(use_site: Use) -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 8]);
    fixture.function(
        &[Type::I1, Type::I1, Type::I32],
        &[Type::I32],
        |mut body| {
            let first = body.parameter::<I1>(0)?;
            let second = body.parameter::<I1>(1)?;
            let input = body.parameter::<I32>(2)?;
            let number = first.select(&input, input.add(1));
            let square = number.mul(&number);
            match use_site {
                Use::Descendant => {
                    body.if_else(
                        first,
                        |mut arm| arm.store(memory, 0, &square),
                        |mut arm| arm.store(memory, 0, &square),
                    )?;
                    body.if_(second, |arm| arm.return_(square.add(11)))?;
                    body.return_(square.add(17))
                }
                Use::Sibling => {
                    // Placement visits the otherwise arm first. Its inner join
                    // must not supply a value to the taken sibling.
                    body.if_else(
                        second,
                        |mut arm| arm.store(memory, 4, square.add(11)),
                        |mut arm| {
                            arm.if_else(
                                first,
                                |mut arm| arm.store(memory, 0, &square),
                                |mut arm| arm.store(memory, 0, &square),
                            )?;
                            arm.store(memory, 4, square.add(17))
                        },
                    )?;
                    body.return_(square.add(23))
                }
            }
        },
    )
}

struct Case {
    arguments: [i32; 3],
    result: i32,
    memory: Vec<u8>,
}

fn cases(use_site: Use) -> impl Iterator<Item = Case> {
    [0, 1].into_iter().flat_map(move |first| {
        [0, 1].into_iter().flat_map(move |second| {
            [0_i32, 5, -2, i32::MAX].into_iter().map(move |input| {
                let number = input.wrapping_add(i32::from(first == 0));
                let square = number.wrapping_mul(number);
                let tail = square.wrapping_add(if second != 0 { 11 } else { 17 });
                let (result, stores) = match use_site {
                    Use::Descendant => (tail, [square, 0]),
                    Use::Sibling => (
                        square.wrapping_add(23),
                        [if second != 0 { 0 } else { square }, tail],
                    ),
                };
                Case {
                    arguments: [first, second, input],
                    result,
                    memory: stores.into_iter().flat_map(i32::to_le_bytes).collect(),
                }
            })
        })
    })
}

fn folded_store(v8: bool) {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 4]);
    let module = fixture.function(
        &[Type::I1, Type::I32, Type::I32],
        &[Type::I32],
        |mut body| {
            let mode = body.parameter::<I1>(0)?;
            let input = body.parameter::<I32>(1)?;
            let factor = body.parameter::<I32>(2)?;
            let product = mode.select(input.add(1), &input).mul(&factor);
            body.if_(&mode, |arm| arm.return_(0))?;
            body.if_(mode.eq(0), |mut arm| arm.store(memory, 0, product))?;
            body.return_(input.mul(factor))
        },
    );
    assert_eq!(count(&module, |op| matches!(op, Operator::I32Mul)), 1);
    for mode in [0, 1] {
        for (input, factor) in [(5_i32, 7_i32), (-2, 3), (i32::MAX, 2)] {
            let result = if mode == 0 {
                input.wrapping_mul(factor)
            } else {
                0
            };
            let memory = result.to_le_bytes();
            if v8 {
                assert_eq!(
                    module.run_v8(
                        &Input::call("run", &[mode, input, factor].map(Value::I32))
                            .with_memories(&[MemoryBytes::new("state", &[0; 4])])
                    ),
                    Observation::returned(&[Value::I32(result)])
                        .with_memories(&[MemoryBytes::new("state", &memory)])
                );
            } else {
                let mut instance = module.instantiate();
                assert_eq!(instance.call::<i32>((mode, input, factor)), Ok(result));
                assert_eq!(&instance.memory("state")[..4], memory);
            }
        }
    }
}

#[test]
fn joined_values_preserve_descendant_and_sibling_results() {
    for use_site in [Use::Descendant, Use::Sibling] {
        let module = joined_square(use_site);
        for case in cases(use_site) {
            let [first, second, input] = case.arguments;
            let mut instance = module.instantiate();
            assert_eq!(
                instance.call::<i32>((first, second, input)),
                Ok(case.result)
            );
            assert_eq!(&instance.memory("state")[..8], case.memory);
        }
    }
}

#[test]
fn a_folded_store_branch_reuses_its_product_after_the_join() {
    folded_store(false);
}

#[test]
#[ignore = "requires Node.js with V8"]
fn v8_joined_values_preserve_results_and_publication() {
    folded_store(true);
    for use_site in [Use::Descendant, Use::Sibling] {
        let module = joined_square(use_site);
        for case in cases(use_site) {
            assert_eq!(
                module.run_v8(
                    &Input::call("run", &case.arguments.map(Value::I32))
                        .with_memories(&[MemoryBytes::new("state", &[0; 8])])
                ),
                Observation::returned(&[Value::I32(case.result)])
                    .with_memories(&[MemoryBytes::new("state", &case.memory)])
            );
        }
    }
}
