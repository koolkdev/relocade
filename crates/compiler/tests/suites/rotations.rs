use crate::fixture::Fixture;
use crate::wasm::{Input, MemoryBytes, Observation, TestModule, Value};
use wasm86_compiler::{AtLeast, IntType, Type, Val, I1, I16, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

fn operators(bytes: &[u8]) -> Vec<Operator<'_>> {
    Validator::new().validate_all(bytes).unwrap();
    Parser::new(0)
        .parse_all(bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(
                body.get_operators_reader()
                    .unwrap()
                    .into_iter()
                    .map(Result::unwrap),
            ),
            _ => None,
        })
        .flatten()
        .collect()
}

// Input, count, left result, right result. Both end bits expose wrapping direction.
const BIT: &[(u64, u32, u64, u64)] = &[
    (1, 0, 1, 1),
    (1, 1, 1, 1),
    (1, 2, 1, 1),
    (1, u32::MAX, 1, 1),
    (0, 31, 0, 0),
];
const BYTE: &[(u64, u32, u64, u64)] = &[
    (0x81, 0, 0x81, 0x81),
    (0x81, 1, 0x03, 0xc0),
    (0x81, 8, 0x81, 0x81),
    (0x81, 16, 0x81, 0x81),
    (0x81, 33, 0x03, 0xc0),
    (0x81, u32::MAX, 0xc0, 0x03),
    (0xff, 3, 0xff, 0xff),
    (0, 3, 0, 0),
];
const WORD: &[(u64, u32, u64, u64)] = &[
    (0x8001, 0, 0x8001, 0x8001),
    (0x8001, 1, 0x0003, 0xc000),
    (0x8001, 4, 0x0018, 0x1800),
    (0x8001, 16, 0x8001, 0x8001),
    (0x8001, 32, 0x8001, 0x8001),
    (0x8001, u32::MAX, 0xc000, 0x0003),
];
const DWORD: &[(u64, u32, u64, u64)] = &[
    (0x8000_0001, 0, 0x8000_0001, 0x8000_0001),
    (0x8000_0001, 1, 0x0000_0003, 0xc000_0000),
    (0x8000_0001, 16, 0x0001_8000, 0x0001_8000),
    (0x8000_0001, 32, 0x8000_0001, 0x8000_0001),
    (0x8000_0001, 64, 0x8000_0001, 0x8000_0001),
    (0x8000_0001, u32::MAX, 0xc000_0000, 0x0000_0003),
];
const QWORD: &[(u64, u32, u64, u64)] = &[
    (
        0x8000_0000_0000_0001,
        0,
        0x8000_0000_0000_0001,
        0x8000_0000_0000_0001,
    ),
    (0x8000_0000_0000_0001, 1, 3, 0xc000_0000_0000_0000),
    (
        0x8000_0000_0000_0001,
        32,
        0x0000_0001_8000_0000,
        0x0000_0001_8000_0000,
    ),
    (
        0x8000_0000_0000_0001,
        64,
        0x8000_0000_0000_0001,
        0x8000_0000_0000_0001,
    ),
    (
        0x8000_0000_0000_0001,
        128,
        0x8000_0000_0000_0001,
        0x8000_0000_0000_0001,
    ),
    (0x8000_0000_0000_0001, u32::MAX, 0xc000_0000_0000_0000, 3),
];

#[test]
fn constant_rotations_fold_with_logical_width_counts() {
    fn check<T: IntType>(cases: &[(u64, u32, u64, u64)])
    where
        I64: AtLeast<T>,
    {
        for &(input, count, left, right) in cases {
            for admitted in [false, true] {
                for (rotate_left, expected) in [(true, left), (false, right)] {
                    let module = Fixture::new().expression(&[], |body| {
                        let input = Val::<I64>::from(input).truncate::<T>();
                        let input = if admitted {
                            body.value(input).unwrap()
                        } else {
                            input
                        };
                        let result = if rotate_left {
                            input.rotl(count)
                        } else {
                            input.rotr(count)
                        };
                        result.unsigned().extend::<I64>()
                    });
                    assert!(
                        matches!(operators(module.bytes()).as_slice(),
                        [Operator::I64Const { value }, Operator::Return, Operator::End]
                            if *value == expected as i64),
                        "{:?}, count={count}, left={rotate_left}, admitted={admitted}",
                        T::TYPE
                    );
                }
            }
        }
    }
    check::<I1>(BIT);
    check::<I8>(BYTE);
    check::<I16>(WORD);
    check::<I32>(DWORD);
    check::<I64>(QWORD);
}

#[test]
fn rotations_accept_literal_and_computed_counts_at_every_logical_width() {
    fn check<T: IntType>(cases: &[(u64, u32, u64, u64)])
    where
        I64: AtLeast<T>,
    {
        for rotate_left in [false, true] {
            let module = Fixture::new().expression(&[T::TYPE, Type::I32], |body| {
                let input = body.parameter::<T>(0).unwrap();
                let count = body.parameter::<I32>(1).unwrap();
                let result = if rotate_left {
                    input.rotl(&count)
                } else {
                    input.rotr(&count)
                };
                result.unsigned().extend::<I64>()
            });
            let mut instance = module.instantiate();
            for &(input, count, left, right) in cases {
                let expected = if rotate_left { left } else { right };
                let input = if T::TYPE == Type::I64 {
                    Value::I64(input as i64)
                } else {
                    Value::I32(input as i32)
                };
                assert_eq!(
                    instance
                        .call_values("run", &[input, Value::I32(count as i32)])
                        .unwrap(),
                    vec![Value::I64(expected as i64)],
                    "{:?}, count={count}, left={rotate_left}",
                    T::TYPE,
                );
                let literal_count = Fixture::new().expression(&[T::TYPE], |body| {
                    let input = body.parameter::<T>(0).unwrap();
                    let result = if rotate_left {
                        input.rotl(count)
                    } else {
                        input.rotr(count)
                    };
                    result.unsigned().extend::<I64>()
                });
                assert_eq!(
                    literal_count
                        .instantiate()
                        .call_values("run", &[input])
                        .unwrap(),
                    vec![Value::I64(expected as i64)],
                    "{:?}, literal count={count}, left={rotate_left}",
                    T::TYPE
                );
            }
        }
    }
    check::<I1>(BIT);
    check::<I8>(BYTE);
    check::<I16>(WORD);
    check::<I32>(DWORD);
    check::<I64>(QWORD);
}

#[test]
fn narrow_rotations_ignore_dirty_carrier_bits_and_normalize_observations() {
    fn check<T: IntType>(cases: &[(i32, i32, i32, i32)])
    where
        I32: AtLeast<T>,
    {
        for &(input, count, left, right) in cases {
            for constant_count in [false, true] {
                for rotate_left in [false, true] {
                    let module = Fixture::new().expression(&[Type::I32; 2], |body| {
                        let input = body.parameter::<I32>(0).unwrap().truncate::<T>().add(1);
                        let count = if constant_count {
                            count.into()
                        } else {
                            body.parameter::<I32>(1).unwrap()
                        };
                        let result = if rotate_left {
                            input.rotl(&count)
                        } else {
                            input.rotr(&count)
                        };
                        result.unsigned().extend::<I32>()
                    });
                    assert_eq!(module.instantiate().call::<i32>((input, count)).unwrap(),
                        if rotate_left { left } else { right },
                        "{:?}, input={input:#x}, count={count}, left={rotate_left}, constant={constant_count}", T::TYPE);
                }
            }
        }
    }
    check::<I1>(&[(2, 0, 1, 1), (2, 31, 1, 1), (3, 32, 0, 0)]);
    check::<I8>(&[
        (0x1234_0080, 1, 0x03, 0xc0),
        (0x1234_0080, 0, 0x81, 0x81),
        (0x1234_0080, 8, 0x81, 0x81),
        (0x1234_00ff, 3, 0, 0),
        (-2, 31, 0xff, 0xff),
    ]);
    check::<I16>(&[
        (0x1234_8000, 4, 0x0018, 0x1800),
        (0x1234_8000, 16, 0x8001, 0x8001),
        (0x1234_ffff, 3, 0, 0),
        (-2, 31, 0xffff, 0xffff),
    ]);
}

#[test]
fn full_width_rotations_use_native_operations_and_share_repeated_values() {
    fn check<T: IntType>() {
        for rotate_left in [false, true] {
            let module = Fixture::new().expression(&[T::TYPE, Type::I32], |body| {
                let input = body.parameter::<T>(0).unwrap();
                let count = body.parameter::<I32>(1).unwrap().add(1);
                let first = if rotate_left {
                    input.rotl(&count)
                } else {
                    input.rotr(&count)
                };
                let second = if rotate_left {
                    input.rotl(&count)
                } else {
                    input.rotr(&count)
                };
                assert!(first.same_expression(&second));
                first.add(second)
            });
            let ops = operators(module.bytes());
            let rotations = ops
                .iter()
                .filter(|op| {
                    matches!(
                        (T::TYPE, rotate_left, op),
                        (Type::I32, true, Operator::I32Rotl)
                            | (Type::I32, false, Operator::I32Rotr)
                            | (Type::I64, true, Operator::I64Rotl)
                            | (Type::I64, false, Operator::I64Rotr)
                    )
                })
                .count();
            let additions = ops
                .iter()
                .filter(|op| matches!(op, Operator::I32Add | Operator::I64Add))
                .count();
            let writes = ops
                .iter()
                .filter(|op| matches!(op, Operator::LocalSet { .. } | Operator::LocalTee { .. }))
                .count();
            assert_eq!((rotations, additions, writes), (1, 2, 1));
        }
    }
    check::<I32>();
    check::<I64>();
}

#[test]
fn invariant_rotations_discard_unused_count_reads() {
    fn check<T: IntType>(mask: u64)
    where
        I64: AtLeast<T>,
    {
        for bits in [0, mask] {
            for rotate_left in [false, true] {
                let mut fixture = Fixture::new();
                let memory = fixture.memory("state", &[17, 0, 0, 0]);
                let module = fixture.function(&[], &[Type::I64], |mut body| {
                    let count = body.load::<I32>(memory, 0)?;
                    let input = Val::<I64>::from(bits).truncate::<T>();
                    let result = if rotate_left {
                        input.rotl(count)
                    } else {
                        input.rotr(count)
                    };
                    body.return_(result.unsigned().extend::<I64>())
                });
                assert!(matches!(operators(module.bytes()).as_slice(),
                    [Operator::I64Const { value }, Operator::Return, Operator::End] if *value == bits as i64));
            }
        }
    }
    check::<I1>(1);
    check::<I8>(0xff);
    check::<I16>(0xffff);
    check::<I32>(0xffff_ffff);
    check::<I64>(u64::MAX);
}

fn count_snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[1, 0, 0, 0, 0x81, 0xa5, 0x5a]);
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let branch = body.parameter::<I1>(0)?;
        let input = body.load::<I8>(memory, 4)?;
        let count = body.load::<I32>(memory, 0)?;
        let left = input.rotl(&count);
        let right = input.rotr(&count);
        let result = body.if_value::<I32>(
            branch,
            |mut arm| {
                arm.store::<I32>(memory, 0, 8)?;
                arm.store::<I8>(memory, 4, 0x55)?;
                arm.yield_(left.unsigned().extend::<I32>().add(&count))
            },
            |mut arm| {
                arm.store::<I32>(memory, 0, 16)?;
                arm.store::<I8>(memory, 4, 0x66)?;
                arm.yield_(right.unsigned().extend::<I32>().add(&count))
            },
        )?;
        body.return_(result)
    })
}

#[test]
fn rotations_preserve_operand_and_count_snapshots_across_branch_writes() {
    let module = count_snapshot();
    for (branch, expected, count, byte) in [(1, 4, 8, 0x55), (0, 0xc1, 16, 0x66)] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((branch,)).unwrap(), expected);
        assert_eq!(
            &instance.memory("state")[..7],
            &[count, 0, 0, 0, byte, 0xa5, 0x5a]
        );
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn computed_rotations_and_count_snapshots_execute_in_v8() {
    let module = count_snapshot();
    for (branch, expected, count, byte) in [(1, 4, 8, 0x55), (0, 0xc1, 16, 0x66)] {
        assert_eq!(
            module.run_v8(
                &Input::call("run", &[Value::I32(branch)])
                    .with_memories(&[MemoryBytes::new("state", &[1, 0, 0, 0, 0x81, 0xa5, 0x5a])])
            ),
            Observation::returned(&[Value::I32(expected)]).with_memories(&[MemoryBytes::new(
                "state",
                &[count, 0, 0, 0, byte, 0xa5, 0x5a]
            )])
        );
    }
    for rotate_left in [false, true] {
        let module = Fixture::new().expression(&[Type::I64, Type::I32], |body| {
            let input = body.parameter::<I64>(0).unwrap();
            let count = body.parameter::<I32>(1).unwrap();
            if rotate_left {
                input.rotl(count)
            } else {
                input.rotr(count)
            }
        });
        for &(input, count, left, right) in QWORD {
            let expected = if rotate_left { left } else { right };
            assert_eq!(
                module.run_v8(&Input::call(
                    "run",
                    &[Value::I64(input as i64), Value::I32(count as i32)]
                )),
                Observation::returned(&[Value::I64(expected as i64)])
            );
        }
    }
}
