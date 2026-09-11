use crate::fixture::Fixture;
use crate::wasm::{Input, MemoryBytes, Observation, TestModule, Value};
use wasm86_compiler::{Type, Val, I1, I32};
use wasmparser::{Operator, Parser, Payload};

fn count_snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0x10, 0, 0, 0]);
    fixture.function(&[], &[Type::I32; 3], |mut body| {
        let input = body.load::<I32>(memory, 0)?;
        let ones = input.popcnt();
        let leading = input.clz();
        let trailing = input.ctz();
        assert!(ones.same_expression(&input.popcnt()));
        assert!(leading.same_expression(&input.clz()));
        assert!(trailing.same_expression(&input.ctz()));
        assert!(!leading.same_expression(&trailing));
        body.store::<I32>(memory, 0, 0)?;
        body.return_((
            ones.add(&ones),
            leading.add(&leading),
            trailing.add(&trailing),
        ))
    })
}

#[test]
fn shared_counts_keep_their_input_snapshot_across_a_store() {
    let module = count_snapshot();
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<(i32, i32, i32)>(()).unwrap(), (2, 54, 8));
    assert_eq!(&instance.memory("state")[..4], &[0; 4]);
    let mut loads = 0;
    let mut counts = [0; 3];
    for payload in Parser::new(0).parse_all(module.bytes()) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for operator in body.get_operators_reader().unwrap() {
                match operator.unwrap() {
                    Operator::I32Load { .. } => loads += 1,
                    Operator::I32Popcnt => counts[0] += 1,
                    Operator::I32Clz => counts[1] += 1,
                    Operator::I32Ctz => counts[2] += 1,
                    _ => {}
                }
            }
        }
    }
    assert_eq!((loads, counts), (1, [1; 3]));
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn shared_counts_keep_their_input_snapshot_in_v8() {
    let module = count_snapshot();
    assert_eq!(
        module.run_v8(
            &Input::call("run", &[]).with_memories(&[MemoryBytes::new("state", &[0x10, 0, 0, 0])])
        ),
        Observation::returned(&[Value::I32(2), Value::I32(54), Value::I32(8)])
            .with_memories(&[MemoryBytes::new("state", &[0; 4])])
    );
}

#[test]
fn counts_recompute_inside_exclusive_arms() {
    for (count, expected) in [
        (Val::<I32>::popcnt as fn(&Val<I32>) -> Val<I32>, 1),
        (Val::<I32>::clz, 27),
        (Val::<I32>::ctz, 4),
    ] {
        let module = Fixture::new().function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
            let condition = body.parameter::<I1>(0)?;
            let input = count(&body.parameter::<I32>(1)?);
            let result = body.if_value::<I32>(
                condition,
                |arm| arm.yield_(input.add(1)),
                |arm| arm.yield_(input.add(2)),
            )?;
            body.return_(result)
        });
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((1, 16)).unwrap(), expected + 1);
        assert_eq!(instance.call::<i32>((0, 16)).unwrap(), expected + 2);
        let mut inside_arm = false;
        let mut counts = 0;
        for payload in Parser::new(0).parse_all(module.bytes()) {
            if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                for operator in body.get_operators_reader().unwrap() {
                    match operator.unwrap() {
                        Operator::If { .. } => inside_arm = true,
                        Operator::End => inside_arm = false,
                        Operator::I32Popcnt | Operator::I32Clz | Operator::I32Ctz => {
                            assert!(inside_arm, "counts belong in their consuming arms");
                            counts += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
        assert_eq!(counts, 2);
    }
}

#[test]
fn count_bounds_normalize_later_one_bit_observations() {
    for (count, expected, masks) in [
        (Val::<I32>::popcnt as fn(&Val<I32>) -> Val<I32>, [0, 1], 1),
        (Val::<I32>::clz, [0, 1], 2),
        (Val::<I32>::ctz, [0, 0], 2),
    ] {
        let module = Fixture::new().expression(&[Type::I32], |body| {
            let bit = body.parameter::<I32>(0).unwrap().and(1);
            count(&bit).truncate::<I1>()
        });
        let mut instance = module.instantiate();
        for (input, expected) in [0, 1].into_iter().zip(expected) {
            assert_eq!(instance.call::<i32>((input,)).unwrap(), expected);
        }
        let actual_masks = Parser::new(0)
            .parse_all(module.bytes())
            .filter_map(|payload| match payload.unwrap() {
                Payload::CodeSectionEntry(body) => Some(
                    body.get_operators_reader()
                        .unwrap()
                        .into_iter()
                        .filter(|operator| matches!(operator, Ok(Operator::I32And)))
                        .count(),
                ),
                _ => None,
            })
            .sum::<usize>();
        assert_eq!(actual_masks, masks);
    }
}
