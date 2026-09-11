use crate::fixture::Fixture;
use crate::wasm::{Input, Observation, Value};
use wasm86_compiler::{AtLeast, IntType, Type, Val, I1, I16, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

// Input, population count, leading zeros, trailing zeros.
const BIT: &[(u64, u64, u64, u64)] = &[(0, 0, 1, 1), (1, 1, 0, 0)];
const BYTE: &[(u64, u64, u64, u64)] = &[
    (0, 0, 8, 8),
    (1, 1, 7, 0),
    (0x80, 1, 0, 7),
    (0xff, 8, 0, 0),
    (0x28, 2, 2, 3),
];
const WORD: &[(u64, u64, u64, u64)] = &[
    (0, 0, 16, 16),
    (1, 1, 15, 0),
    (0x8000, 1, 0, 15),
    (0xffff, 16, 0, 0),
    (0x0480, 2, 5, 7),
];
const DWORD: &[(u64, u64, u64, u64)] = &[
    (0, 0, 32, 32),
    (1, 1, 31, 0),
    (0x8000_0000, 1, 0, 31),
    (0xffff_ffff, 32, 0, 0),
    (0x0040_0200, 2, 9, 9),
];
const QWORD: &[(u64, u64, u64, u64)] = &[
    (0, 0, 64, 64),
    (1, 1, 63, 0),
    (0x8000_0000_0000_0000, 1, 0, 63),
    (u64::MAX, 64, 0, 0),
    (0x0000_0100_0000_0200, 2, 23, 9),
    (0x0000_0001_0000_0000, 1, 31, 32),
];

fn carrier<T: IntType>(bits: u64) -> Value {
    if T::TYPE == Type::I64 {
        Value::I64(bits as i64)
    } else {
        Value::I32(bits as i32)
    }
}

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
fn literal_and_admitted_bit_counts_fold_at_every_logical_width() {
    fn check<T: IntType>(cases: &[(u64, u64, u64, u64)])
    where
        I64: AtLeast<T>,
    {
        for &(bits, ones, leading, trailing) in cases {
            for admitted in [false, true] {
                let module = Fixture::new().function(&[], &[T::TYPE; 3], |body| {
                    let input = Val::<I64>::from(bits).truncate::<T>();
                    let input = if admitted { body.value(input)? } else { input };
                    body.return_((input.popcnt(), input.clz(), input.ctz()))
                });
                let expected = [ones, leading, trailing].map(carrier::<T>);
                let code = operators(module.bytes());
                assert_eq!(code.len(), 5);
                for (operator, expected) in code.iter().take(3).zip(expected) {
                    assert!(
                        matches!((operator, expected),
                            (Operator::I32Const { value }, Value::I32(expected)) if *value == expected)
                            || matches!((operator, expected),
                            (Operator::I64Const { value }, Value::I64(expected)) if *value == expected),
                        "{:?}, input={bits:#x}, admitted={admitted}",
                        T::TYPE
                    );
                }
                assert!(matches!(code[3..], [Operator::Return, Operator::End]));
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
fn runtime_bit_counts_return_the_receivers_logical_type() {
    fn check<T: IntType>(cases: &[(u64, u64, u64, u64)]) {
        let module = Fixture::new().function(&[T::TYPE], &[T::TYPE; 3], |body| {
            let input = body.parameter::<T>(0)?;
            body.return_((input.popcnt(), input.clz(), input.ctz()))
        });
        let mut instance = module.instantiate();
        for &(bits, ones, leading, trailing) in cases {
            assert_eq!(
                instance.call_values("run", &[carrier::<T>(bits)]).unwrap(),
                [ones, leading, trailing].map(carrier::<T>),
                "{:?}, input={bits:#x}",
                T::TYPE
            );
        }
        let code = operators(module.bytes());
        let native = code
            .iter()
            .filter(|operator| {
                matches!(
                    operator,
                    Operator::I32Popcnt
                        | Operator::I64Popcnt
                        | Operator::I32Clz
                        | Operator::I64Clz
                        | Operator::I32Ctz
                        | Operator::I64Ctz
                )
            })
            .count();
        assert_eq!(native, 3);
        assert!(!code.iter().any(|op| matches!(op, Operator::I32And)));
    }
    check::<I1>(BIT);
    check::<I8>(BYTE);
    check::<I16>(WORD);
    check::<I32>(DWORD);
    check::<I64>(QWORD);
}

#[test]
fn narrow_counts_share_normalization_of_dirty_carrier_bits() {
    fn check<T: IntType>(cases: &[(i32, i32, i32, i32)])
    where
        I32: AtLeast<T>,
    {
        let module = Fixture::new().function(&[Type::I32], &[T::TYPE; 3], |body| {
            let input = body.parameter::<I32>(0)?.truncate::<T>().add(1);
            body.return_((input.popcnt(), input.clz(), input.ctz()))
        });
        let mut instance = module.instantiate();
        for &(bits, ones, leading, trailing) in cases {
            assert_eq!(
                instance.call::<(i32, i32, i32)>((bits,)).unwrap(),
                (ones, leading, trailing),
                "{:?}, input={bits:#x}",
                T::TYPE
            );
        }
        assert_eq!(
            operators(module.bytes())
                .iter()
                .filter(|op| matches!(op, Operator::I32And))
                .count(),
            1,
            "all three observers share one normalization"
        );
    }
    check::<I1>(&[(2, 1, 0, 0), (3, 0, 1, 1), (-1, 0, 1, 1)]);
    check::<I8>(&[
        (0x1234_0080, 2, 0, 0),
        (0x1234_00ff, 0, 8, 8),
        (-2, 8, 0, 0),
        (0x4000_0000, 1, 7, 0),
        (0x4000_007f, 1, 0, 7),
    ]);
    check::<I16>(&[
        (0x1234_8000, 2, 0, 0),
        (0x1234_ffff, 0, 16, 16),
        (-2, 16, 0, 0),
        (0x4000_0000, 1, 15, 0),
        (0x4000_7fff, 1, 0, 15),
    ]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn logical_zero_after_narrow_overflow_and_wide_counts_execute_in_v8() {
    let module = Fixture::new().function(&[Type::I32], &[Type::I16; 3], |body| {
        let input = body.parameter::<I32>(0)?.truncate::<I16>().add(1);
        body.return_((input.popcnt(), input.clz(), input.ctz()))
    });
    assert_eq!(
        module.run_v8(&Input::call("run", &[Value::I32(0x1234_ffff)])),
        Observation::returned(&[Value::I32(0), Value::I32(16), Value::I32(16)])
    );
    let module = Fixture::new().function(&[Type::I64], &[Type::I64; 3], |body| {
        let input = body.parameter::<I64>(0)?;
        body.return_((input.popcnt(), input.clz(), input.ctz()))
    });
    for &(input, ones, leading, trailing) in QWORD {
        assert_eq!(
            module.run_v8(&Input::call("run", &[Value::I64(input as i64)])),
            Observation::returned(&[ones, leading, trailing].map(carrier::<I64>))
        );
    }
}
