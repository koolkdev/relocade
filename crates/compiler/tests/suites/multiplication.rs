use crate::fixture::Fixture;
use crate::wasm::{Input, Observation, TestModule, Value};
use wasm86_compiler::{AtLeast, IntType, Type, Val, I1, I16, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

#[path = "multiplication/observations.rs"]
mod observations;

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

// Left operand, right operand, logical low product.
const BIT: &[(u64, u64, u64)] = &[(1, 1, 1), (1, 0, 0), (0, 1, 0)];
const BYTE: &[(u64, u64, u64)] = &[
    (0xff, 0xff, 1),
    (0x80, 2, 0),
    (0x7f, 3, 0x7d),
    (0x81, 0xff, 0x7f),
    (0, 0xff, 0),
];
const WORD: &[(u64, u64, u64)] = &[
    (0xffff, 0xffff, 1),
    (0x8000, 2, 0),
    (0x8001, 3, 0x8003),
    (0x1234, 0x10, 0x2340),
];
const DWORD: &[(u64, u64, u64)] = &[
    (0xffff_ffff, 0xffff_ffff, 1),
    (0x8000_0000, 2, 0),
    (0x8000_0001, 3, 0x8000_0003),
    (0x1234_5678, 0x10, 0x2345_6780),
];
const QWORD: &[(u64, u64, u64)] = &[
    (u64::MAX, u64::MAX, 1),
    (0x8000_0000_0000_0000, 2, 0),
    (0x8000_0000_0000_0001, 3, 0x8000_0000_0000_0003),
    (0xffff_ffff, 0xffff_ffff, 0xffff_fffe_0000_0001),
    (0x0123_4567_89ab_cdef, 0x10, 0x1234_5678_9abc_def0),
];

fn input<T: IntType>(bits: u64) -> Value {
    if T::TYPE == Type::I64 {
        Value::I64(bits as i64)
    } else {
        Value::I32(bits as i32)
    }
}

fn computed_product<T: IntType>() -> TestModule
where
    I64: AtLeast<T>,
{
    Fixture::new().expression(&[T::TYPE; 2], |body| {
        let left = body.parameter::<T>(0).unwrap();
        let right = body.parameter::<T>(1).unwrap();
        left.mul(right).unsigned().extend::<I64>()
    })
}

#[test]
fn constant_products_fold_at_every_logical_width() {
    fn check<T: IntType>(cases: &[(u64, u64, u64)])
    where
        I64: AtLeast<T>,
    {
        for &(left, right, expected) in cases {
            for admitted in [false, true] {
                let module = Fixture::new().expression(&[], |body| {
                    let left = Val::<I64>::from(left).truncate::<T>();
                    let right = Val::<I64>::from(right).truncate::<T>();
                    let product = if admitted {
                        body.value(left).unwrap().mul(body.value(right).unwrap())
                    } else {
                        left.mul(right)
                    };
                    product.unsigned().extend::<I64>()
                });
                assert!(
                    matches!(operators(module.bytes()).as_slice(),
                        [Operator::I64Const { value }, Operator::Return, Operator::End]
                        if *value == expected as i64),
                    "{:?}: {left:#x} * {right:#x}, admitted={admitted}",
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
fn computed_and_literal_products_wrap_at_every_logical_width() {
    fn check<T: IntType>(cases: &[(u64, u64, u64)])
    where
        I64: AtLeast<T>,
    {
        let module = computed_product::<T>();
        let mut instance = module.instantiate();
        for &(left, right, expected) in cases {
            assert_eq!(
                instance
                    .call_values("run", &[input::<T>(left), input::<T>(right)])
                    .unwrap(),
                vec![Value::I64(expected as i64)],
                "{:?}: {left:#x} * {right:#x}",
                T::TYPE
            );
            let literal = Fixture::new().expression(&[T::TYPE], |body| {
                body.parameter::<T>(0)
                    .unwrap()
                    .mul(Val::<I64>::from(right).truncate::<T>())
                    .unsigned()
                    .extend::<I64>()
            });
            assert_eq!(
                literal
                    .instantiate()
                    .call_values("run", &[input::<T>(left)])
                    .unwrap(),
                vec![Value::I64(expected as i64)]
            );
        }
    }
    check::<I1>(BIT);
    check::<I8>(BYTE);
    check::<I16>(WORD);
    check::<I32>(DWORD);
    check::<I64>(QWORD);
}

#[test]
fn products_accept_native_signed_unsigned_and_boolean_literals() {
    let module = Fixture::new().function(
        &[Type::I64, Type::I1],
        &[Type::I64, Type::I64, Type::I64, Type::I1],
        |body| {
            let value = body.parameter::<I64>(0)?;
            let bit = body.parameter::<I1>(1)?;
            body.return_((
                value.mul(-1),
                value.mul(u32::MAX),
                value.mul(0x8000_0000_0000_0000_u64),
                bit.mul(true),
            ))
        },
    );
    assert_eq!(
        module
            .instantiate()
            .call::<(i64, i64, i64, i32)>((3_i64, 1))
            .unwrap(),
        (-3, 12_884_901_885, i64::MIN, 1)
    );
}

#[test]
fn zero_and_one_products_fold_before_emission() {
    fn check<T: IntType>()
    where
        I64: AtLeast<T>,
    {
        for zero_on_left in [false, true] {
            let mut fixture = Fixture::new();
            let memory = fixture.memory("state", &[7; 8]);
            let module = fixture.function(&[], &[Type::I64], |mut body| {
                let value = body.load::<I64>(memory, 0)?.truncate::<T>();
                assert!(value.mul(1).same_expression(&value));
                assert!(Val::<T>::from(1).mul(&value).same_expression(&value));
                let zero = if zero_on_left {
                    Val::<T>::from(0).mul(value)
                } else {
                    value.mul(0)
                };
                body.return_(zero.unsigned().extend::<I64>())
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
    }
    check::<I1>();
    check::<I8>();
    check::<I16>();
    check::<I32>();
    check::<I64>();
}

#[test]
fn repeated_products_share_native_multiplication() {
    fn check<T: IntType>() {
        let module = Fixture::new().expression(&[T::TYPE; 2], |body| {
            let left = body.parameter::<T>(0).unwrap();
            let right = body.parameter::<T>(1).unwrap();
            let first = left.mul(&right);
            let second = left.mul(right);
            assert!(first.same_expression(&second));
            first.add(second)
        });
        let ops = operators(module.bytes());
        assert_eq!(
            ops.iter()
                .filter(|op| matches!(
                    (T::TYPE, op),
                    (Type::I32, Operator::I32Mul) | (Type::I64, Operator::I64Mul)
                ))
                .count(),
            1
        );
    }
    check::<I32>();
    check::<I64>();
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn computed_products_execute_in_v8_at_every_logical_width() {
    fn check<T: IntType>(cases: &[(u64, u64, u64)])
    where
        I64: AtLeast<T>,
    {
        let module = computed_product::<T>();
        for &(left, right, expected) in cases {
            assert_eq!(
                module.run_v8(&Input::call("run", &[input::<T>(left), input::<T>(right)])),
                Observation::returned(&[Value::I64(expected as i64)])
            );
        }
    }
    check::<I1>(BIT);
    check::<I8>(BYTE);
    check::<I16>(WORD);
    check::<I32>(DWORD);
    check::<I64>(QWORD);
}
