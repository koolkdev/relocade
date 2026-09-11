use crate::fixture::Fixture;
use crate::wasm::{Input, Observation, TestModule, Value};
use wasm86_compiler::{AtLeast, IntType, Type, Val, I1, I16, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

#[path = "division/execution.rs"]
mod execution;
#[path = "division/validation.rs"]
mod validation;

#[derive(Clone, Copy, Debug)]
enum Kind {
    DivUnsigned,
    DivSigned,
    RemUnsigned,
    RemSigned,
}

const KINDS: [Kind; 4] = [
    Kind::DivUnsigned,
    Kind::DivSigned,
    Kind::RemUnsigned,
    Kind::RemSigned,
];

impl Kind {
    fn apply<T: IntType>(self, left: impl Into<Val<T>>, right: impl Into<Val<T>>) -> Val<T> {
        let left = left.into();
        match self {
            Self::DivUnsigned => left.unsigned().div(right),
            Self::DivSigned => left.signed().div(right),
            Self::RemUnsigned => left.unsigned().rem(right),
            Self::RemSigned => left.signed().rem(right),
        }
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

fn input<T: IntType>(bits: u64) -> Value {
    if T::TYPE == Type::I64 {
        Value::I64(bits as i64)
    } else {
        Value::I32(bits as i32)
    }
}

struct Case {
    left: u64,
    right: u64,
    // Unsigned quotient, signed quotient, unsigned remainder, signed remainder.
    expected: [u64; 4],
}

const BIT: &[Case] = &[
    Case {
        left: 0,
        right: 1,
        expected: [0, 0, 0, 0],
    },
    Case {
        left: 1,
        right: 1,
        expected: [1, 1, 0, 0],
    },
];
const BYTE: &[Case] = &[
    Case {
        left: 128,
        right: 255,
        expected: [0, 128, 128, 0],
    },
    Case {
        left: 249,
        right: 3,
        expected: [83, 254, 0, 255],
    },
    Case {
        left: 7,
        right: 253,
        expected: [0, 254, 7, 1],
    },
    Case {
        left: 249,
        right: 253,
        expected: [0, 2, 249, 255],
    },
    Case {
        left: 128,
        right: 2,
        expected: [64, 192, 0, 0],
    },
];
const WORD: &[Case] = &[
    Case {
        left: 32768,
        right: 65535,
        expected: [0, 32768, 32768, 0],
    },
    Case {
        left: 65529,
        right: 3,
        expected: [21843, 65534, 0, 65535],
    },
    Case {
        left: 7,
        right: 65533,
        expected: [0, 65534, 7, 1],
    },
    Case {
        left: 32768,
        right: 2,
        expected: [16384, 49152, 0, 0],
    },
];
const DWORD: &[Case] = &[
    Case {
        left: 0xffff_fff9,
        right: 3,
        expected: [1_431_655_763, 0xffff_fffe, 0, 0xffff_ffff],
    },
    Case {
        left: 7,
        right: 0xffff_fffd,
        expected: [0, 0xffff_fffe, 7, 1],
    },
    Case {
        left: 0x8000_0000,
        right: 2,
        expected: [0x4000_0000, 0xc000_0000, 0, 0],
    },
];
const QWORD: &[Case] = &[
    Case {
        left: 0xffff_ffff_ffff_fff9,
        right: 3,
        expected: [
            6_148_914_691_236_517_203,
            0xffff_ffff_ffff_fffe,
            0,
            u64::MAX,
        ],
    },
    Case {
        left: 7,
        right: 0xffff_ffff_ffff_fffd,
        expected: [0, 0xffff_ffff_ffff_fffe, 7, 1],
    },
    Case {
        left: 0x8000_0000_0000_0000,
        right: 2,
        expected: [0x4000_0000_0000_0000, 0xc000_0000_0000_0000, 0, 0],
    },
];

fn computed<T: IntType>() -> TestModule {
    Fixture::new().function(&[T::TYPE; 2], &[T::TYPE; 4], |body| {
        let left = body.parameter::<T>(0)?;
        let right = body.parameter::<T>(1)?;
        let uq = left.unsigned().div(&right);
        let sq = left.signed().div(&right);
        let ur = left.unsigned().rem(&right);
        let sr = left.signed().rem(&right);
        body.return_((uq, sq, ur, sr))
    })
}

#[test]
fn computed_division_and_remainder_use_logical_signedness_at_every_width() {
    fn check<T: IntType>(cases: &[Case]) {
        let module = computed::<T>();
        let mut instance = module.instantiate();
        for case in cases {
            assert_eq!(
                instance
                    .call_values("run", &[input::<T>(case.left), input::<T>(case.right)])
                    .unwrap(),
                case.expected.map(input::<T>),
                "{:?}: {} / {}",
                T::TYPE,
                case.left,
                case.right
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
fn successful_constant_division_folds_to_the_independently_expected_value() {
    fn check<T: IntType>(cases: &[Case])
    where
        I64: AtLeast<T>,
    {
        for case in cases {
            for (kind, expected) in KINDS.into_iter().zip(case.expected) {
                for admitted in [false, true] {
                    let module = Fixture::new().function(&[], &[Type::I64], |body| {
                        let mut left = Val::<I64>::from(case.left).truncate::<T>();
                        let mut right = Val::<I64>::from(case.right).truncate::<T>();
                        if admitted {
                            left = body.value(left)?;
                            right = body.value(right)?;
                        }
                        let result = kind.apply(left, right);
                        body.return_(result.unsigned().extend::<I64>())
                    });
                    assert!(
                        matches!(operators(module.bytes()).as_slice(),
                        [Operator::I64Const { value }, Operator::Return, Operator::End] if *value == expected as i64),
                        "{kind:?} {:?}, admitted={admitted}",
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
fn native_carriers_use_all_four_wasm_opcodes() {
    for (module, wide) in [(computed::<I32>(), false), (computed::<I64>(), true)] {
        let names: Vec<_> = operators(module.bytes())
            .iter()
            .filter_map(|op| match op {
                Operator::I32DivU if !wide => Some("div_u"),
                Operator::I64DivU if wide => Some("div_u"),
                Operator::I32DivS if !wide => Some("div_s"),
                Operator::I64DivS if wide => Some("div_s"),
                Operator::I32RemU if !wide => Some("rem_u"),
                Operator::I64RemU if wide => Some("rem_u"),
                Operator::I32RemS if !wide => Some("rem_s"),
                Operator::I64RemS if wide => Some("rem_s"),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["div_u", "div_s", "rem_u", "rem_s"]);
    }
}

#[test]
fn native_literals_keep_their_signed_unsigned_and_boolean_meanings() {
    let module = Fixture::new().function(
        &[Type::I64, Type::I1],
        &[Type::I64, Type::I64, Type::I64, Type::I1],
        |body| {
            let value = body.parameter::<I64>(0)?;
            let bit = body.parameter::<I1>(1)?;
            let signed = value.signed().div(-1);
            let unsigned = value.unsigned().div(u32::MAX);
            let remainder = value.unsigned().rem(0x8000_0000_0000_0000_u64);
            let boolean = bit.unsigned().div(true);
            body.return_((signed, unsigned, remainder, boolean))
        },
    );
    assert_eq!(
        module
            .instantiate()
            .call::<(i64, i64, i64, i32)>((0x1_0000_0000_i64, 1))
            .unwrap(),
        (-0x1_0000_0000, 1, 0x1_0000_0000, 1)
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn logical_division_and_remainder_execute_in_v8_at_every_width() {
    fn check<T: IntType>(case: &Case) {
        assert_eq!(
            computed::<T>().run_v8(&Input::call(
                "run",
                &[input::<T>(case.left), input::<T>(case.right)]
            )),
            Observation::returned(&case.expected.map(input::<T>))
        );
    }
    check::<I1>(&BIT[0]);
    check::<I8>(&BYTE[0]);
    check::<I16>(&WORD[0]);
    check::<I32>(&DWORD[0]);
    check::<I64>(&QWORD[0]);
}
