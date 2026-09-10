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

#[test]
fn dynamic_shifts_share_the_count_and_shifted_value() {
    for direction in ["left", "unsigned right", "signed right"] {
        let module = Fixture::new().expression(&[Type::I32; 2], |body| {
            let value = body.parameter::<I32>(0).unwrap();
            let count = body.parameter::<I32>(1).unwrap().add(1);
            let shifted = match direction {
                "left" => value.shl(count),
                "unsigned right" => value.unsigned().shr(count),
                _ => value.signed().shr(count),
            };
            shifted.add(&shifted)
        });
        let ops = operators(module.bytes());
        let shifts = ops
            .iter()
            .filter(|op| matches!(op, Operator::I32Shl | Operator::I32ShrU | Operator::I32ShrS))
            .count();
        let adds = ops
            .iter()
            .filter(|op| matches!(op, Operator::I32Add))
            .count();
        let writes = ops
            .iter()
            .filter(|op| matches!(op, Operator::LocalSet { .. } | Operator::LocalTee { .. }))
            .count();
        assert_eq!((shifts, adds, writes), (1, 2, 1), "{direction}");
    }
}

#[test]
fn zero_shifts_do_not_force_unused_count_loads() {
    for direction in ["left", "unsigned right", "signed right"] {
        let mut fixture = Fixture::new();
        let memory = fixture.memory("state", &[7, 0, 0, 0]);
        let module = fixture.function(&[], &[Type::I32], |mut body| {
            let count = body.load::<I32>(memory, 65536)?;
            let zero = Val::<I32>::from(0);
            let shifted = match direction {
                "left" => zero.shl(count),
                "unsigned right" => zero.unsigned().shr(count),
                _ => zero.signed().shr(count),
            };
            body.return_(shifted)
        });
        assert!(
            matches!(
                operators(module.bytes()).as_slice(),
                [
                    Operator::I32Const { value: 0 },
                    Operator::Return,
                    Operator::End
                ]
            ),
            "{direction}"
        );
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(()).unwrap(), 0, "{direction}");
        assert_eq!(&instance.memory("state")[..4], &[7, 0, 0, 0]);
        assert!(instance.callbacks().is_empty());
    }
}

#[test]
fn constant_right_shifts_fold_the_logical_sign_and_carrier_count() {
    fn check<T: IntType>(input: Val<T>, count: u32, unsigned: u64, signed: u64)
    where
        I64: AtLeast<T>,
    {
        for admitted in [false, true] {
            for (is_signed, expected) in [(false, unsigned), (true, signed)] {
                let module = Fixture::new().expression(&[], |body| {
                    let input = if admitted {
                        body.value(&input).unwrap()
                    } else {
                        input.clone()
                    };
                    let result = if is_signed {
                        input.signed().shr(count)
                    } else {
                        input.unsigned().shr(count)
                    };
                    result.unsigned().extend::<I64>()
                });
                assert!(
                    matches!(
                        operators(module.bytes()).as_slice(),
                        [Operator::I64Const { value }, Operator::Return, Operator::End]
                            if *value == expected as i64
                    ),
                    "{:?}, count={count}, signed={is_signed}, admitted={admitted}",
                    T::TYPE
                );
            }
        }
    }
    check::<I1>(true.into(), 1, 0, 1);
    check::<I1>(true.into(), 32, 1, 1);
    check::<I8>(0x80.into(), 1, 0x40, 0xc0);
    check::<I8>(0xfe.into(), 8, 0, 0xff);
    check::<I8>(0x7f.into(), 31, 0, 0);
    check::<I8>(0x81.into(), 32, 0x81, 0x81);
    check::<I16>(0x8001.into(), 16, 0, 0xffff);
    check::<I32>(0x8000_0001u32.into(), 1, 0x4000_0000, 0xc000_0000);
    check::<I32>(0x8000_0001u32.into(), 32, 0x8000_0001, 0x8000_0001);
    check::<I64>(
        0x8000_0000_0000_0001u64.into(),
        32,
        0x8000_0000,
        0xffff_ffff_8000_0000,
    );
    check::<I64>(0x8000_0000_0000_0001u64.into(), 63, 1, u64::MAX);
    check::<I64>(
        0x8000_0000_0000_0001u64.into(),
        64,
        0x8000_0000_0000_0001,
        0x8000_0000_0000_0001,
    );
    check::<I64>(
        0x8000_0000_0000_0001u64.into(),
        65,
        0x4000_0000_0000_0000,
        0xc000_0000_0000_0000,
    );
    check::<I64>(0x8000_0000_0000_0001u64.into(), u32::MAX, 1, u64::MAX);
}

#[test]
fn computed_right_shifts_use_i32_counts_at_every_logical_width() {
    fn check<T: IntType>(cases: &[(u64, u32, u64, u64)])
    where
        I64: AtLeast<T>,
    {
        for is_signed in [false, true] {
            let module = Fixture::new().expression(&[T::TYPE, Type::I32], |body| {
                let input = body.parameter::<T>(0).unwrap();
                let count = body.parameter::<I32>(1).unwrap();
                let result = if is_signed {
                    input.signed().shr(&count)
                } else {
                    input.unsigned().shr(&count)
                };
                result.unsigned().extend::<I64>()
            });
            let mut instance = module.instantiate();
            for &(input, count, unsigned, signed) in cases {
                let input = if T::TYPE == Type::I64 {
                    Value::I64(input as i64)
                } else {
                    Value::I32(input as i32)
                };
                let expected = if is_signed { signed } else { unsigned };
                assert_eq!(
                    instance
                        .call_values("run", &[input, Value::I32(count as i32)])
                        .unwrap(),
                    vec![Value::I64(expected as i64)],
                    "{:?}, count={count}, signed={is_signed}",
                    T::TYPE,
                );
            }
        }
    }
    check::<I1>(&[(1, 0, 1, 1), (1, 1, 0, 1), (1, 32, 1, 1), (0, 31, 0, 0)]);
    check::<I8>(&[
        (0x80, 1, 0x40, 0xc0),
        (0xfe, 8, 0, 0xff),
        (0x7f, 31, 0, 0),
        (0x81, 32, 0x81, 0x81),
    ]);
    check::<I16>(&[
        (0x8001, 1, 0x4000, 0xc000),
        (0x8001, 16, 0, 0xffff),
        (0x7fff, 31, 0, 0),
        (0x8001, 32, 0x8001, 0x8001),
    ]);
    check::<I32>(&[
        (0x8000_0001, 1, 0x4000_0000, 0xc000_0000),
        (0x8000_0001, 31, 1, 0xffff_ffff),
        (0x8000_0001, 32, 0x8000_0001, 0x8000_0001),
        (0x8000_0001, u32::MAX, 1, 0xffff_ffff),
    ]);
    check::<I64>(&[
        (
            0x8000_0000_0000_0001,
            32,
            0x8000_0000,
            0xffff_ffff_8000_0000,
        ),
        (0x8000_0000_0000_0001, 63, 1, u64::MAX),
        (
            0x8000_0000_0000_0001,
            64,
            0x8000_0000_0000_0001,
            0x8000_0000_0000_0001,
        ),
        (
            0x8000_0000_0000_0001,
            65,
            0x4000_0000_0000_0000,
            0xc000_0000_0000_0000,
        ),
        (0x8000_0000_0000_0001, u32::MAX, 1, u64::MAX),
    ]);
}

#[test]
fn narrow_right_shifts_ignore_dirty_upper_bits_before_and_after_shifting() {
    fn check<T: IntType>(cases: &[(i32, i32, i32, i32)])
    where
        I32: AtLeast<T>,
    {
        for is_signed in [false, true] {
            for constant_count in [false, true] {
                for &(input, count, unsigned, signed) in cases {
                    let module = Fixture::new().expression(&[Type::I32; 2], |body| {
                        // Truncation and addition leave arbitrary upper carrier bits.
                        let value = body.parameter::<I32>(0).unwrap().truncate::<T>().add(1);
                        let count = if constant_count {
                            count.into()
                        } else {
                            body.parameter::<I32>(1).unwrap()
                        };
                        let shifted = if is_signed {
                            value.signed().shr(&count)
                        } else {
                            value.unsigned().shr(&count)
                        };
                        shifted.unsigned().extend::<I32>()
                    });
                    assert_eq!(
                        module.instantiate().call::<i32>((input, count)).unwrap(),
                        if is_signed { signed } else { unsigned },
                        "{:?}, input={input:#x}, count={count}, signed={is_signed}, constant={constant_count}", T::TYPE,
                    );
                }
            }
        }
    }
    check::<I1>(&[(2, 31, 0, 1), (3, 2, 0, 0), (2, 32, 1, 1)]);
    check::<I8>(&[
        (0x1234_007f, 1, 0x40, 0xc0),
        (0x1234_00ff, 1, 0, 0),
        (0x1234_007f, 8, 0, 0xff),
        (0x1234_007f, 0, 0x80, 0x80),
        (0x1234_007f, 32, 0x80, 0x80),
        (-2, 8, 0, 0xff),
    ]);
    check::<I16>(&[
        (0x1234_7fff, 4, 0x0800, 0xf800),
        (0x1234_ffff, 8, 0, 0),
        (0x1234_7fff, 32, 0x8000, 0x8000),
        (-2, 16, 0, 0xffff),
    ]);
}

fn count_snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[1, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let branch = body.parameter::<I1>(0)?;
        let input = body.parameter::<I32>(1)?.truncate::<I8>();
        let count = body.load::<I32>(memory, 0)?;
        let shifted = input.signed().shr(&count);
        let result = body.if_value::<I32>(
            branch,
            |mut arm| {
                arm.store::<I32>(memory, 0, 8)?;
                arm.yield_(shifted.unsigned().extend::<I32>().add(&count))
            },
            |mut arm| {
                arm.store::<I32>(memory, 0, 32)?;
                arm.yield_(input.unsigned().shr(&count).unsigned().extend::<I32>())
            },
        )?;
        body.return_(result)
    })
}

#[test]
fn computed_counts_keep_their_memory_snapshot_across_branch_writes() {
    let module = count_snapshot();
    for (branch, expected, stored) in [(1, 0xc1, 8), (0, 0x40, 32)] {
        let mut instance = module.instantiate();
        assert_eq!(
            instance.call::<i32>((branch, 0x1234_0080)).unwrap(),
            expected
        );
        assert_eq!(
            &instance.memory("state")[..6],
            &[stored, 0, 0, 0, 0xa5, 0x5a]
        );
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn computed_right_shifts_and_count_snapshots_execute_in_v8() {
    let module = count_snapshot();
    for (branch, expected, stored) in [(1, 0xc1, 8), (0, 0x40, 32)] {
        assert_eq!(
            module.run_v8(
                &Input::call("run", &[Value::I32(branch), Value::I32(0x1234_0080)])
                    .with_memories(&[MemoryBytes::new("state", &[1, 0, 0, 0, 0xa5, 0x5a])]),
            ),
            Observation::returned(&[Value::I32(expected)])
                .with_memories(&[MemoryBytes::new("state", &[stored, 0, 0, 0, 0xa5, 0x5a])]),
        );
    }
    let module = Fixture::new().expression(&[Type::I64, Type::I32], |body| {
        body.parameter::<I64>(0)
            .unwrap()
            .signed()
            .shr(body.parameter::<I32>(1).unwrap())
    });
    for (count, expected) in [(32, -2147483648i64), (63, -1), (64, i64::MIN + 1)] {
        assert_eq!(
            module.run_v8(&Input::call(
                "run",
                &[Value::I64(i64::MIN + 1), Value::I32(count)]
            )),
            Observation::returned(&[Value::I64(expected)]),
        );
    }
}

#[test]
fn truncation_follows_the_wrapped_shift_count() {
    let module = Fixture::new().expression(&[Type::I32], |b| {
        b.parameter::<I32>(0)
            .unwrap()
            .unsigned()
            .shr(33)
            .truncate::<I8>()
    });
    assert_eq!(module.instantiate().call::<i32>((-1,)).unwrap(), 255);
}

#[test]
fn dword_left_shifts_wrap_the_count() {
    for (count, argument, expected) in [(31, 1, -2147483648), (33, 1, 2)] {
        let module =
            Fixture::new().expression(&[Type::I32], |b| b.parameter::<I32>(0).unwrap().shl(count));
        assert_eq!(
            module.instantiate().call::<i32>((argument,)).unwrap(),
            expected,
            "shift by {count}"
        );
    }
}

#[test]
fn dword_right_shifts_wrap_the_count() {
    for (count, argument, expected) in [(31, -2147483648, 1), (32, -2147483648, -2147483648)] {
        let module = Fixture::new().expression(&[Type::I32], |b| {
            b.parameter::<I32>(0).unwrap().unsigned().shr(count)
        });
        assert_eq!(
            module.instantiate().call::<i32>((argument,)).unwrap(),
            expected,
            "shift by {count}"
        );
    }
}

#[test]
fn qword_left_shifts_wrap_the_count() {
    for (count, argument, expected) in [(63, 1_i64, -9223372036854775808_i64), (65, 1_i64, 2_i64)] {
        let module =
            Fixture::new().expression(&[Type::I64], |b| b.parameter::<I64>(0).unwrap().shl(count));
        assert_eq!(
            module.instantiate().call::<i64>((argument,)).unwrap(),
            expected,
            "shift by {count}"
        );
    }
}

#[test]
fn qword_right_shifts_wrap_the_count() {
    for (count, argument, expected) in [
        (63, -9223372036854775808_i64, 1_i64),
        (64, -9223372036854775808_i64, -9223372036854775808_i64),
    ] {
        let module = Fixture::new().expression(&[Type::I64], |b| {
            b.parameter::<I64>(0).unwrap().unsigned().shr(count)
        });
        assert_eq!(
            module.instantiate().call::<i64>((argument,)).unwrap(),
            expected,
            "shift by {count}"
        );
    }
}

#[test]
fn byte_left_shifts_wrap_the_carrier_count_and_result() {
    for (count, argument, expected) in [(1, 128, 0), (8, 128, 0), (32, 128, 128)] {
        let module =
            Fixture::new().expression(&[Type::I8], |b| b.parameter::<I8>(0).unwrap().shl(count));
        assert_eq!(
            module.instantiate().call::<i32>((argument,)).unwrap(),
            expected,
            "shift by {count}"
        );
    }
}

#[test]
fn byte_right_shifts_use_the_carrier_count() {
    for (count, argument, expected) in [(8, 128, 0), (32, 128, 128)] {
        let module = Fixture::new().expression(&[Type::I8], |b| {
            b.parameter::<I8>(0).unwrap().unsigned().shr(count)
        });
        assert_eq!(
            module.instantiate().call::<i32>((argument,)).unwrap(),
            expected,
            "shift by {count}"
        );
    }
}

#[test]
fn narrow_right_shifts_observe_the_wrapped_sum() {
    fn check<T: IntType>(cases: &[(i32, i32)]) {
        let module = Fixture::new().expression(&[T::TYPE], |b| {
            b.parameter::<T>(0).unwrap().add(1).unsigned().shr(1)
        });
        let mut instance = module.instantiate();
        for &(argument, expected) in cases {
            assert_eq!(
                instance.call::<i32>((argument,)).unwrap(),
                expected,
                "{argument}"
            );
        }
    }
    check::<I1>(&[(1, 0)]);
    check::<I8>(&[(255, 0), (127, 64)]);
    check::<I16>(&[(65535, 0)]);
}
