use crate::fixture::Fixture;
use crate::wasm::{TestModule, Value};

use wasm86_compiler::{
    FunctionBuilder, IntType, Program, Signature, Type, Val, I1, I16, I32, I64, I8,
};
use wasmparser::{Operator, Parser, Payload, Validator};

#[path = "signed_arithmetic/representation.rs"]
mod representation;

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
fn wrapping_difference_uses_one_subtraction_and_signed_carrier_comparison() {
    let module = Fixture::new().expression(&[Type::I32; 2], |body| {
        let left = body.parameter::<I32>(0).unwrap();
        let right = body.parameter::<I32>(1).unwrap();
        left.sub(right).signed().ge(0)
    });
    assert!(matches!(
        operators(module.bytes()).as_slice(),
        [
            Operator::LocalGet { local_index: 0 },
            Operator::LocalGet { local_index: 1 },
            Operator::I32Sub,
            Operator::I32Const { value: 0 },
            Operator::I32GeS,
            Operator::Return,
            Operator::End,
        ]
    ));
}

#[test]
fn constant_subtraction_and_comparisons_fold_at_the_logical_width() {
    for (module, expected) in [
        (
            Fixture::new().expression(&[], |_| Val::<I8>::from(0).sub(1).signed().lt(0)),
            1,
        ),
        (
            Fixture::new().expression(&[], |_| Val::<I8>::from(0).sub(129).signed().lt(0)),
            0,
        ),
        (
            Fixture::new().expression(&[], |_| Val::<I16>::from(0).sub(32769).signed().ge(0)),
            1,
        ),
        (
            Fixture::new().expression(&[], |_| {
                Val::<I32>::from(0x8000_0000u32).sub(1).signed().ge(0)
            }),
            1,
        ),
        (
            Fixture::new().expression(&[], |_| Val::<I64>::from(0).sub(1).signed().ge(0)),
            0,
        ),
        (
            Fixture::new().expression(&[], |_| Val::<I1>::from(false).sub(true).signed().lt(false)),
            1,
        ),
        (
            Fixture::new().expression(&[Type::I32], |b| {
                let value = b.parameter::<I32>(0).unwrap();
                value.sub(&value).signed().ge(0)
            }),
            1,
        ),
    ] {
        assert!(matches!(
            operators(module.bytes()).as_slice(),
            [Operator::I32Const { value }, Operator::Return, Operator::End] if *value == expected
        ));
    }
}

#[test]
fn signed_arithmetic_wraps_and_compares_at_logical_widths() {
    fn function<T: IntType>(
        program: &mut Program,
        name: &str,
        parameters: &[Type],
        build: impl FnOnce(&FunctionBuilder<'_>) -> Val<T>,
    ) {
        let function = program
            .function(
                Signature {
                    parameters: parameters.to_vec(),
                    results: vec![T::TYPE],
                },
                |body| {
                    let result = build(&body);
                    body.return_(result)
                },
            )
            .unwrap();
        program.export(name, function).unwrap();
    }

    fn arithmetic<T: IntType>(program: &mut Program, suffix: &str) {
        let parameters = &[T::TYPE; 2];
        function(program, &format!("sub{suffix}"), parameters, |body| {
            body.parameter::<T>(0)
                .unwrap()
                .sub(body.parameter::<T>(1).unwrap())
        });
        function(program, &format!("lt{suffix}"), parameters, |body| {
            body.parameter::<T>(0)
                .unwrap()
                .signed()
                .lt(body.parameter::<T>(1).unwrap())
        });
        function(
            program,
            &format!("difference_nonnegative{suffix}"),
            parameters,
            |body| {
                body.parameter::<T>(0)
                    .unwrap()
                    .sub(body.parameter::<T>(1).unwrap())
                    .signed()
                    .ge(0)
            },
        );
    }

    let mut program = Program::new();
    arithmetic::<I1>(&mut program, "1");
    arithmetic::<I8>(&mut program, "8");
    arithmetic::<I16>(&mut program, "16");
    arithmetic::<I32>(&mut program, "32");
    arithmetic::<I64>(&mut program, "64");
    let module = TestModule::new(&program.compile().unwrap());
    let mut instance = module.instantiate();
    for (name, left, right, expected) in [
        ("sub1", Value::I32(0), Value::I32(1), Value::I32(1)),
        ("sub8", Value::I32(0), Value::I32(1), Value::I32(255)),
        ("sub16", Value::I32(0), Value::I32(1), Value::I32(65535)),
        (
            "sub32",
            Value::I32(-2147483648),
            Value::I32(1),
            Value::I32(2147483647),
        ),
        (
            "sub32",
            Value::I32(2147483647),
            Value::I32(-1),
            Value::I32(-2147483648),
        ),
        (
            "sub64",
            Value::I64(-9223372036854775808),
            Value::I64(1),
            Value::I64(9223372036854775807),
        ),
        (
            "sub64",
            Value::I64(9223372036854775807),
            Value::I64(-1),
            Value::I64(-9223372036854775808),
        ),
        ("lt1", Value::I32(1), Value::I32(0), Value::I32(1)),
        ("lt1", Value::I32(0), Value::I32(1), Value::I32(0)),
        ("lt8", Value::I32(128), Value::I32(127), Value::I32(1)),
        ("lt8", Value::I32(127), Value::I32(255), Value::I32(0)),
        ("lt16", Value::I32(32768), Value::I32(32767), Value::I32(1)),
        (
            "lt32",
            Value::I32(-2147483648),
            Value::I32(2147483647),
            Value::I32(1),
        ),
        (
            "lt64",
            Value::I64(-9223372036854775808),
            Value::I64(9223372036854775807),
            Value::I32(1),
        ),
        (
            "difference_nonnegative1",
            Value::I32(0),
            Value::I32(1),
            Value::I32(0),
        ),
        (
            "difference_nonnegative1",
            Value::I32(1),
            Value::I32(1),
            Value::I32(1),
        ),
        (
            "difference_nonnegative8",
            Value::I32(0),
            Value::I32(1),
            Value::I32(0),
        ),
        (
            "difference_nonnegative8",
            Value::I32(0),
            Value::I32(129),
            Value::I32(1),
        ),
        (
            "difference_nonnegative8",
            Value::I32(128),
            Value::I32(1),
            Value::I32(1),
        ),
        (
            "difference_nonnegative16",
            Value::I32(0),
            Value::I32(32769),
            Value::I32(1),
        ),
        (
            "difference_nonnegative16",
            Value::I32(32768),
            Value::I32(1),
            Value::I32(1),
        ),
        (
            "difference_nonnegative32",
            Value::I32(-1),
            Value::I32(1),
            Value::I32(0),
        ),
        (
            "difference_nonnegative32",
            Value::I32(0),
            Value::I32(-1),
            Value::I32(1),
        ),
        (
            "difference_nonnegative32",
            Value::I32(7),
            Value::I32(7),
            Value::I32(1),
        ),
        (
            "difference_nonnegative32",
            Value::I32(7),
            Value::I32(8),
            Value::I32(0),
        ),
        (
            "difference_nonnegative32",
            Value::I32(-2147483648),
            Value::I32(1),
            Value::I32(1),
        ),
        (
            "difference_nonnegative64",
            Value::I64(0),
            Value::I64(1),
            Value::I32(0),
        ),
        (
            "difference_nonnegative64",
            Value::I64(-9223372036854775808),
            Value::I64(1),
            Value::I32(1),
        ),
    ] {
        assert_eq!(
            instance.call_values(name, &[left, right]).unwrap(),
            vec![expected],
            "{name}({left:?}, {right:?})"
        );
    }
}

#[test]
fn underflow_shares_raw_store_bits_with_signed_and_unsigned_observers() {
    for (initial, expected, stored) in [([0, 0xa5], 511, [0xff, 0xa5]), ([7, 0xa5], 6, [6, 0xa5])] {
        let mut fixture = Fixture::new();
        let memory = fixture.memory("state", &initial);
        let module = fixture.function(&[], &[Type::I32], |mut body| {
            let difference = body.load::<I8>(memory, 0)?.sub(1);
            body.store(memory, 0, &difference)?;
            let negative = difference.signed().lt(0).unsigned().extend::<I32>();
            body.return_(difference.unsigned().extend::<I32>().add(negative.shl(8)))
        });
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(()).unwrap(), expected);
        assert_eq!(&instance.memory("state")[..2], &stored);
        let operations = operators(module.bytes());
        let relevant: Vec<_> = operations
            .iter()
            .filter_map(|operator| match operator {
                Operator::I32Load8U { .. } => Some("read"),
                Operator::I32Add | Operator::I32Sub => Some("arithmetic"),
                Operator::I32Store8 { .. } => Some("store"),
                Operator::I32And => Some("unsigned low bits"),
                Operator::I32Extend8S => Some("signed low bits"),
                _ => None,
            })
            .collect();
        assert_eq!(
            relevant,
            [
                "read",
                "arithmetic",
                "store",
                "unsigned low bits",
                "signed low bits",
                "arithmetic"
            ]
        );
    }
}
