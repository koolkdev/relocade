use super::operators;
use crate::fixture::{signature, Fixture};
use crate::wasm::{Call, Callback, Input, MemoryBytes, Observation, TestModule, Value};
use wasm86_compiler::{AtLeast, IntType, Type, I1, I16, I32, I8};
use wasmparser::Operator;

struct NarrowCase {
    arguments: [i32; 2],
    // Logical result, unsigned and signed widening, zero, negative, below one, half.
    returned: [i32; 7],
    memory: [u8; 3],
}

const BIT: &[NarrowCase] = &[
    NarrowCase {
        arguments: [0, 0],
        returned: [1, 1, -1, 0, 1, 0, 0],
        memory: [1, 0x5a, 0xc3],
    },
    NarrowCase {
        arguments: [1, 2],
        returned: [0, 0, 0, 1, 0, 1, 0],
        memory: [0, 0x5a, 0xc3],
    },
];
const BYTE: &[NarrowCase] = &[
    NarrowCase {
        arguments: [0x1234_0000, 0x1234_0002],
        returned: [253, 253, -3, 0, 1, 0, 126],
        memory: [0xfd, 0x5a, 0xc3],
    },
    NarrowCase {
        arguments: [0x1234_0081, 1],
        returned: [0, 0, 0, 1, 0, 1, 0],
        memory: [0, 0x5a, 0xc3],
    },
    NarrowCase {
        arguments: [0x100, 0xff],
        returned: [0, 0, 0, 1, 0, 1, 0],
        memory: [0, 0x5a, 0xc3],
    },
];
const WORD: &[NarrowCase] = &[
    NarrowCase {
        arguments: [0x1234_0000, 2],
        returned: [65533, 65533, -3, 0, 1, 0, 32766],
        memory: [0xfd, 0xff, 0xc3],
    },
    NarrowCase {
        arguments: [0x1234_8001, 1],
        returned: [0, 0, 0, 1, 0, 1, 0],
        memory: [0, 0, 0xc3],
    },
];
const INITIAL: &[u8] = &[0xa5, 0x5a, 0xc3];

fn narrow_product<T: IntType>() -> TestModule
where
    I32: AtLeast<T>,
{
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", INITIAL);
    let receive = fixture.callback("receive", signature(&[T::TYPE], &[]), &[]);
    fixture.function(
        &[Type::I32; 2],
        &[
            T::TYPE,
            Type::I32,
            Type::I32,
            Type::I1,
            Type::I1,
            Type::I1,
            Type::I32,
        ],
        |mut body| {
            let left = body.parameter::<I32>(0)?.truncate::<T>().sub(1);
            let right = body.parameter::<I32>(1)?.truncate::<T>().add(1);
            let product = left.mul(right);
            let stored = product.unsigned().extend::<I32>();
            if T::TYPE == Type::I16 {
                body.store(memory, 0, stored.truncate::<I16>())?;
            } else {
                body.store(memory, 0, stored.truncate::<I8>())?;
            }
            body.call::<()>(receive, &[product.argument()])?;
            body.return_((
                &product,
                product.unsigned().extend::<I32>(),
                product.signed().extend::<I32>(),
                product.eq(0),
                product.signed().lt(0),
                product.unsigned().lt(1),
                product.unsigned().shr(1).unsigned().extend::<I32>(),
            ))
        },
    )
}

fn narrow_observation(case: &NarrowCase) -> Observation {
    let memory = MemoryBytes::new("state", &case.memory);
    Observation::returned(&case.returned.map(Value::I32))
        .with_callbacks(&[Call::new("receive", &[Value::I32(case.returned[0])])
            .with_memories(std::slice::from_ref(&memory))])
        .with_memories(&[memory])
}

#[test]
fn narrow_products_observe_logical_bits_after_dirty_carrier_arithmetic() {
    fn check<T: IntType>(cases: &[NarrowCase])
    where
        I32: AtLeast<T>,
    {
        let module = narrow_product::<T>();
        for case in cases {
            let mut instance = module.instantiate();
            let expected = narrow_observation(case);
            assert_eq!(
                instance
                    .call_values("run", &case.arguments.map(Value::I32))
                    .unwrap(),
                case.returned.map(Value::I32)
            );
            assert_eq!(instance.callbacks(), expected.callbacks);
            assert_eq!(&instance.memory("state")[..3], case.memory);
        }
    }
    check::<I1>(BIT);
    check::<I8>(BYTE);
    check::<I16>(WORD);
}

#[test]
fn multiplication_bounds_mask_only_products_that_can_exceed_the_logical_width() {
    for (mask, expected) in [
        (0x0f, vec!["and", "and", "multiply"]),
        (0xff, vec!["multiply", "and"]),
    ] {
        let module = Fixture::new().expression(&[Type::I8; 2], |body| {
            let left = body.parameter::<I8>(0).unwrap().and(mask);
            let right = body.parameter::<I8>(1).unwrap().and(mask);
            left.mul(right).unsigned().extend::<I32>()
        });
        let ops = operators(module.bytes());
        let relevant: Vec<_> = ops
            .iter()
            .filter_map(|op| match op {
                Operator::I32Mul => Some("multiply"),
                Operator::I32And => Some("and"),
                _ => None,
            })
            .collect();
        assert_eq!(relevant, expected);
    }
}

const SNAPSHOT_INITIAL: &[u8] = &[0xff, 0xa5, 0x5a, 0xc3, 1, 1, 0, 0];
const SNAPSHOT_CASES: &[(i32, i32, [u8; 8])] = &[
    (1, 65790, [2, 0xa5, 0x5a, 0xc3, 7, 0, 0, 0]),
    (0, 65792, [3, 0xa5, 0x5a, 0xc3, 11, 0, 0, 0]),
];

fn product_snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", SNAPSHOT_INITIAL);
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let branch = body.parameter::<I1>(0)?;
        let left = body.load::<I8>(memory, 0)?.unsigned().extend::<I32>();
        let right = body.load::<I32>(memory, 4)?;
        let product = left.mul(&right);
        let result = body.if_value::<I32>(
            branch,
            |mut arm| {
                arm.store::<I8>(memory, 0, 2)?;
                arm.store::<I32>(memory, 4, 7)?;
                arm.yield_(product.add(&left))
            },
            |mut arm| {
                arm.store::<I8>(memory, 0, 3)?;
                arm.store::<I32>(memory, 4, 11)?;
                arm.yield_(product.add(&right))
            },
        )?;
        body.return_(result)
    })
}

#[test]
fn products_preserve_both_operand_snapshots_across_branch_writes() {
    let module = product_snapshot();
    for &(branch, expected, memory) in SNAPSHOT_CASES {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((branch,)).unwrap(), expected);
        assert_eq!(&instance.memory("state")[..8], memory);
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn narrow_products_and_operand_snapshots_execute_in_v8() {
    fn check<T: IntType>(cases: &[NarrowCase])
    where
        I32: AtLeast<T>,
    {
        let module = narrow_product::<T>();
        for case in cases {
            assert_eq!(
                module.run_v8(
                    &Input::call("run", &case.arguments.map(Value::I32))
                        .with_memories(&[MemoryBytes::new("state", INITIAL)])
                        .with_callbacks(&[Callback::new("receive", &[])])
                ),
                narrow_observation(case)
            );
        }
    }
    check::<I1>(BIT);
    check::<I8>(BYTE);
    check::<I16>(WORD);
    let module = product_snapshot();
    for &(branch, expected, memory) in SNAPSHOT_CASES {
        assert_eq!(
            module.run_v8(
                &Input::call("run", &[Value::I32(branch)])
                    .with_memories(&[MemoryBytes::new("state", SNAPSHOT_INITIAL)])
            ),
            Observation::returned(&[Value::I32(expected)])
                .with_memories(&[MemoryBytes::new("state", &memory)])
        );
    }
}
