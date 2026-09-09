use crate::fixture::{signature, Fixture};
use crate::wasm::{Call, MemoryBytes, Value};
use wasm86_compiler::{Type, Val, I16, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

#[path = "integer_ops/addressing.rs"]
mod addressing;
#[path = "integer_ops/scalars.rs"]
mod scalars;

#[derive(Default, Debug)]
struct Code {
    adds: usize,
    masks: usize,
    shifts: usize,
    comparisons: usize,
    zero_tests: usize,
    conversions: usize,
    locals: u32,
    writes: usize,
    accesses: Vec<&'static str>,
    constants: Vec<i32>,
    returns: usize,
}
fn inspect(bytes: &[u8]) -> Code {
    Validator::new().validate_all(bytes).unwrap();
    let mut code = Code::default();
    let mut bodies = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            bodies += 1;
            for local in body.get_locals_reader().unwrap() {
                code.locals += local.unwrap().0;
            }
            let mut ops = body.get_operators_reader().unwrap();
            while !ops.eof() {
                match ops.read().unwrap() {
                    Operator::I32Const { value } => code.constants.push(value),
                    Operator::Return => code.returns += 1,
                    Operator::I32Add | Operator::I64Add => code.adds += 1,
                    Operator::I32And | Operator::I64And => code.masks += 1,
                    Operator::I32Shl | Operator::I32ShrU | Operator::I64Shl | Operator::I64ShrU => {
                        code.shifts += 1
                    }
                    Operator::I32Eq | Operator::I32Ne | Operator::I64Eq | Operator::I64Ne => {
                        code.comparisons += 1
                    }
                    Operator::I32Eqz | Operator::I64Eqz => {
                        code.comparisons += 1;
                        code.zero_tests += 1;
                    }
                    Operator::I32WrapI64 | Operator::I64ExtendI32U => code.conversions += 1,
                    Operator::LocalSet { .. } | Operator::LocalTee { .. } => code.writes += 1,
                    Operator::I32Load8U { .. } => code.accesses.push("load"),
                    Operator::I32Store8 { .. } => code.accesses.push("store"),
                    _ => {}
                }
            }
        }
    }
    assert_eq!(bodies, 1);
    code
}

#[test]
fn high_bit_equality_uses_zero_tests() {
    let module = Fixture::new().expression(&[Type::I32], |b| {
        b.parameter::<I32>(0)
            .unwrap()
            .and(0x8000_0000u32)
            .eq(0x8000_0000u32)
    });
    let code = inspect(module.bytes());
    assert_eq!(
        (
            code.masks,
            code.zero_tests,
            code.comparisons - code.zero_tests
        ),
        (1, 2, 0),
    );
    for (input, expected) in [(0, 0), (-2147483648, 1), (-1, 1)] {
        assert_eq!(
            module.instantiate().call::<i32>((input,)).unwrap(),
            expected
        );
    }
}

#[test]
fn reversed_high_bit_inequality_uses_one_zero_test() {
    let module = Fixture::new().expression(&[Type::I32], |b| {
        let mask = b.value::<I32>(0x8000_0000u32).unwrap();
        mask.ne(mask.and(b.parameter::<I32>(0).unwrap()))
    });
    let code = inspect(module.bytes());
    assert_eq!(
        (
            code.masks,
            code.zero_tests,
            code.comparisons - code.zero_tests
        ),
        (1, 1, 0),
    );
    for (input, expected) in [(0, 1), (-2147483648, 0), (2147483647, 1)] {
        assert_eq!(
            module.instantiate().call::<i32>((input,)).unwrap(),
            expected
        );
    }
}

#[test]
fn wide_reversed_high_bit_equality_uses_zero_tests() {
    let module = Fixture::new().expression(&[Type::I64], |b| {
        let mask = b.value::<I64>(0x8000_0000_0000_0000u64).unwrap();
        mask.eq(b.parameter::<I64>(0).unwrap().and(&mask))
    });
    let code = inspect(module.bytes());
    assert_eq!(
        (
            code.masks,
            code.zero_tests,
            code.comparisons - code.zero_tests
        ),
        (1, 2, 0),
    );
    for (input, expected) in [(0i64, 0), (-9223372036854775808i64, 1)] {
        assert_eq!(
            module.instantiate().call::<i32>((input,)).unwrap(),
            expected
        );
    }
}

#[test]
fn wide_reversed_mask_inequality_uses_one_zero_test() {
    let module = Fixture::new().expression(&[Type::I64], |b| {
        let mask = b.value::<I64>(0x8000_0000_0000_0000u64).unwrap();
        mask.and(b.parameter::<I64>(0).unwrap()).ne(&mask)
    });
    let code = inspect(module.bytes());
    assert_eq!(
        (
            code.masks,
            code.zero_tests,
            code.comparisons - code.zero_tests
        ),
        (1, 1, 0),
    );
    for (input, expected) in [(-9223372036854775808i64, 0), (9223372036854775807i64, 1)] {
        assert_eq!(
            module.instantiate().call::<i32>((input,)).unwrap(),
            expected
        );
    }
}

#[test]
fn mask_comparison_normalizes_overflowing_narrow_input() {
    let module = Fixture::new().expression(&[Type::I8], |b| {
        b.parameter::<I8>(0).unwrap().add(1).and(128).eq(128)
    });
    let code = inspect(module.bytes());
    assert_eq!(
        (
            code.masks,
            code.zero_tests,
            code.comparisons - code.zero_tests
        ),
        (1, 2, 0),
    );
    for (input, expected) in [(127, 1), (255, 0)] {
        assert_eq!(
            module.instantiate().call::<i32>((input,)).unwrap(),
            expected
        );
    }
}

#[test]
fn multiple_bits_require_full_equality() {
    let module = Fixture::new().expression(&[Type::I32], |b| {
        b.parameter::<I32>(0).unwrap().and(3).eq(3)
    });
    let code = inspect(module.bytes());
    assert_eq!(
        (
            code.masks,
            code.zero_tests,
            code.comparisons - code.zero_tests
        ),
        (1, 0, 1),
    );
    for (input, expected) in [(1, 0), (2, 0), (3, 1)] {
        assert_eq!(
            module.instantiate().call::<i32>((input,)).unwrap(),
            expected
        );
    }
}

#[test]
fn dynamic_shifts_share_the_count_and_shifted_value() {
    let module = Fixture::new().expression(&[Type::I32, Type::I32], |b| {
        let count = b.parameter::<I32>(1).unwrap().add(1);
        let shifted = b.parameter::<I32>(0).unwrap().shl(count);
        shifted.add(&shifted)
    });
    let code = inspect(module.bytes());
    assert_eq!(
        (code.shifts, code.adds, code.locals, code.writes),
        (1, 2, 1, 1)
    );
}

#[test]
fn zero_shifts_do_not_force_unused_count_loads() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[7, 0, 0, 0]);
    let module = fixture.function(&[], Some(Type::I32), |mut b| {
        let count = b.load::<I32>(memory, 65536)?;
        let value = Val::<I32>::from(0).shl(count);
        b.return_(value)
    });
    let code = inspect(module.bytes());
    assert!(code.accesses.is_empty());
    assert_eq!(code.shifts, 0);
    assert_eq!(code.constants, [0]);
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 0);
    assert_eq!(&instance.memory("state")[..4], &[7, 0, 0, 0]);
    assert!(instance.callbacks().is_empty());
}

#[test]
fn signed_literal_extension_folds_the_logical_sign_bit() {
    let module = Fixture::new().expression(&[], |_| Val::<I8>::from(255).signed().extend::<I32>());
    let code = inspect(module.bytes());
    assert_eq!(code.constants, [-1]);
    assert_eq!((code.shifts, code.conversions, code.locals), (0, 0, 0));
}

#[test]
fn extraction_shares_its_computed_value() {
    let module = Fixture::new().expression(&[Type::I32], |b| {
        let field = b.parameter::<I32>(0).unwrap().unsigned().shr(8).and(255);
        let _unused = field.or(7);
        field.add(&field)
    });
    let code = inspect(module.bytes());
    assert_eq!(
        (code.shifts, code.masks, code.adds, code.locals, code.writes),
        (1, 1, 1, 1, 1)
    );
}

#[test]
fn widened_predicates_share_their_computed_value() {
    let module = Fixture::new().expression(&[Type::I32; 2], |b| {
        let predicate = b
            .parameter::<I32>(0)
            .unwrap()
            .eq(b.parameter::<I32>(1).unwrap());
        let widened = predicate.unsigned().extend::<I32>();
        widened.add(&widened)
    });
    let code = inspect(module.bytes());
    assert_eq!(
        (
            code.comparisons,
            code.masks,
            code.adds,
            code.locals,
            code.writes
        ),
        (1, 0, 1, 1, 1)
    );
}

#[test]
fn shared_conversions_reuse_their_canonical_value() {
    let module = Fixture::new().expression(&[Type::I8], |b| {
        let byte = b.parameter::<I8>(0).unwrap().and(7);
        let wide = byte.unsigned().extend::<I16>().unsigned().extend::<I32>();
        wide.add(&wide)
    });
    let code = inspect(module.bytes());
    assert_eq!(
        (
            code.masks,
            code.conversions,
            code.adds,
            code.locals,
            code.writes
        ),
        (1, 0, 1, 1, 1)
    );
    assert_eq!(module.instantiate().call::<i32>((255,)).unwrap(), 14);
}

#[test]
fn conversion_roundtrips_preserve_narrow_values_without_redundant_work() {
    let module = Fixture::new().expression(&[Type::I32], |b| {
        let masked = b.parameter::<I32>(0).unwrap().and(255);
        let wide = masked.truncate::<I8>().unsigned().extend::<I32>();
        wide.add(&wide)
    });
    let code = inspect(module.bytes());
    assert_eq!(
        (
            code.masks,
            code.conversions,
            code.adds,
            code.locals,
            code.writes
        ),
        (1, 0, 1, 1, 1)
    );
    assert_eq!(module.instantiate().call::<i32>((305441791,)).unwrap(), 510);
}

#[test]
fn same_width_conversions_emit_no_operations() {
    let module = Fixture::new().expression(&[Type::I32], |b| {
        b.parameter::<I32>(0)
            .unwrap()
            .truncate::<I32>()
            .unsigned()
            .extend::<I32>()
    });
    let code = inspect(module.bytes());
    assert_eq!(
        (code.masks, code.conversions, code.locals, code.writes),
        (0, 0, 0, 0)
    );
}

#[test]
fn converted_loads_preserve_the_snapshot_across_overlapping_stores() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0xff, 0xa5, 0x5a]);
    let module = fixture.function(&[], Some(Type::I32), |mut b| {
        let loaded = b.load::<I8>(memory, 0)?;
        let wide = loaded.unsigned().extend::<I32>();
        let masked = wide.and(7);
        b.store::<I8>(memory, 0, 0)?;
        b.return_(wide.add(&masked))
    });
    let code = inspect(module.bytes());
    assert_eq!(code.accesses, ["load", "store"]);
    assert_eq!(
        (code.masks, code.adds, code.locals, code.writes),
        (1, 1, 1, 1)
    );
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 262);
    assert_eq!(&instance.memory("state")[..3], &[0, 0xa5, 0x5a]);
    assert!(instance.callbacks().is_empty());
}

#[test]
fn narrow_observers_share_normalization_across_store_and_call_boundaries() {
    for (input, result, expected_arguments, expected_byte) in [
        (255, i64::MIN, [0, 0, 1, 0], 0),
        (127, 17, [128, 64, 0, 128], 0x80),
    ] {
        let mut fixture = Fixture::new();
        let memory = fixture.memory("state", &[0xff, 0xa5, 0x5a]);
        let target = fixture.callback(
            "receive",
            signature(&[Type::I32, Type::I8, Type::I1, Type::I32], Some(Type::I64)),
            Some(Value::I64(result)),
        );
        let module = fixture.function(&[Type::I8], Some(Type::I64), |mut b| {
            let raw = b.parameter::<I8>(0)?.add(1);
            b.store(memory, 0, &raw)?;
            let wide = raw.unsigned().extend::<I32>();
            let shifted = raw.unsigned().shr(1);
            let zero = raw.eq(0);
            b.tail_call(
                target,
                &[
                    wide.argument(),
                    shifted.argument(),
                    zero.argument(),
                    wide.argument(),
                ],
            )
        });
        let code = inspect(module.bytes());
        assert_eq!(code.accesses, ["store"]);
        assert_eq!(
            (
                code.masks,
                code.shifts,
                code.comparisons,
                code.adds,
                code.locals,
                code.writes
            ),
            (1, 1, 1, 1, 1, 2)
        );
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i64>((input,)).unwrap(), result);
        let expected_memory = [expected_byte, 0xa5, 0x5a];
        assert_eq!(&instance.memory("state")[..3], &expected_memory);
        assert_eq!(
            instance.callbacks(),
            &[Call::new("receive", &expected_arguments.map(Value::I32))
                .with_memories(&[MemoryBytes::new("state", &expected_memory)])]
        );
    }
}

#[test]
fn constant_integer_operations_fold_before_emission() {
    let module = Fixture::new().expression(&[], |b| {
        b.value::<I64>(0xffff_ffff_1234_5678u64)
            .unwrap()
            .and(0xffff_ffffu64)
            .or(3)
            .shl(65)
            .unsigned()
            .shr(1)
            .truncate::<I32>()
            .eq(0x1234_567b)
    });
    let code = inspect(module.bytes());
    assert_eq!(code.constants, [1]);
    assert_eq!(code.returns, 1);
    assert_eq!(
        (
            code.adds,
            code.masks,
            code.shifts,
            code.comparisons,
            code.conversions,
            code.locals
        ),
        (0, 0, 0, 0, 0, 0)
    );
}

#[test]
fn narrow_comparisons_ignore_unused_carrier_bits() {
    for (ne, zero, a, other, expected) in [
        (false, false, 1, 255, 1),
        (true, false, 1, 255, 0),
        (false, false, 1, 254, 0),
        (true, false, 1, 254, 1),
        (false, true, 255, 0, 1),
        (true, true, 255, 0, 0),
    ] {
        let parameters = if zero {
            vec![Type::I8]
        } else {
            vec![Type::I8; 2]
        };
        let module = Fixture::new().expression(&parameters, |b| {
            let a = b.parameter::<I8>(0).unwrap().add(1);
            let other = if zero {
                let _unused = a.unsigned().extend::<I32>();
                b.value::<I8>(0).unwrap()
            } else {
                b.parameter::<I8>(1).unwrap().add(3)
            };
            if ne {
                a.ne(&other)
            } else {
                a.eq(&other)
            }
        });
        let mut instance = module.instantiate();
        let actual = if zero {
            instance.call::<i32>((a,))
        } else {
            instance.call::<i32>((a, other))
        };
        assert_eq!(actual.unwrap(), expected);
    }
}
