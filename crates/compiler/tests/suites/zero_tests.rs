use crate::fixture::Fixture;
use crate::wasm::{TestModule, Value};
#[path = "zero_tests/conditions.rs"]
mod conditions;

use wasm86_compiler::{FunctionBuilder, Signature, Type, Val, I1, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

struct Case {
    name: &'static str,
    parameter: Type,
    build: fn(&FunctionBuilder<'_>) -> Val<I1>,
    needs_test: bool,
    inputs: &'static [(Value, i32)],
}

fn cases() -> [Case; 10] {
    [
        Case {
            name: "masked32",
            parameter: Type::I32,
            build: |body| body.parameter::<I32>(0).unwrap().and(1).ne(0),
            needs_test: false,
            inputs: &[(Value::I32(0), 0), (Value::I32(2), 0), (Value::I32(3), 1)],
        },
        Case {
            name: "shifted32",
            parameter: Type::I32,
            build: |body| body.parameter::<I32>(0).unwrap().unsigned().shr(31).ne(0),
            needs_test: false,
            inputs: &[(Value::I32(2147483647), 0), (Value::I32(-2147483648), 1)],
        },
        Case {
            name: "masked64",
            parameter: Type::I64,
            build: |body| body.parameter::<I64>(0).unwrap().and(1u64).ne(0u64),
            needs_test: false,
            inputs: &[(Value::I64(0), 0), (Value::I64(2), 0), (Value::I64(3), 1)],
        },
        Case {
            name: "shifted64",
            parameter: Type::I64,
            build: |body| {
                body.parameter::<I64>(0)
                    .unwrap()
                    .unsigned()
                    .shr(63)
                    .ne(0u64)
            },
            needs_test: false,
            inputs: &[
                (Value::I64(9223372036854775807), 0),
                (Value::I64(-9223372036854775808), 1),
            ],
        },
        Case {
            name: "narrow_shift",
            parameter: Type::I8,
            build: |body| {
                body.parameter::<I8>(0)
                    .unwrap()
                    .add(1)
                    .unsigned()
                    .shr(7)
                    .ne(0)
            },
            needs_test: false,
            inputs: &[(Value::I32(127), 1), (Value::I32(255), 0)],
        },
        Case {
            name: "narrow_mask",
            parameter: Type::I8,
            build: |body| body.parameter::<I8>(0).unwrap().add(1).and(1).ne(0),
            needs_test: false,
            inputs: &[(Value::I32(0), 1), (Value::I32(1), 0), (Value::I32(255), 0)],
        },
        Case {
            name: "other_bit",
            parameter: Type::I32,
            build: |body| body.parameter::<I32>(0).unwrap().and(2).ne(0),
            needs_test: true,
            inputs: &[(Value::I32(0), 0), (Value::I32(1), 0), (Value::I32(2), 1)],
        },
        Case {
            name: "wide_nonzero",
            parameter: Type::I64,
            build: |body| body.parameter::<I64>(0).unwrap().ne(0u64),
            needs_test: true,
            inputs: &[
                (Value::I64(0), 0),
                (Value::I64(2), 1),
                (Value::I64(4294967296), 1),
            ],
        },
        Case {
            name: "narrow_nonzero",
            parameter: Type::I8,
            build: |body| body.parameter::<I8>(0).unwrap().add(1).ne(0),
            needs_test: true,
            inputs: &[(Value::I32(0), 1), (Value::I32(1), 1), (Value::I32(255), 0)],
        },
        Case {
            name: "masked_zero",
            parameter: Type::I32,
            build: |body| body.parameter::<I32>(0).unwrap().and(1).eq(0),
            needs_test: true,
            inputs: &[(Value::I32(0), 1), (Value::I32(1), 0), (Value::I32(2), 1)],
        },
    ]
}

fn module(cases: &[Case]) -> TestModule {
    let mut fixture = Fixture::new();
    let program = &mut fixture.program;
    for case in cases {
        let function = program.declare(Signature {
            parameters: vec![case.parameter],
            result: Some(Type::I1),
        });
        let body = program.define(function).unwrap();
        let value = (case.build)(&body);
        body.return_(value).unwrap();
        program.export(case.name, function).unwrap();
    }
    fixture.compile()
}

#[test]
fn proven_zero_or_one_values_need_no_predicate_but_keep_the_i32_result_carrier() {
    let cases = cases();
    let module = module(&cases);
    let bytes = module.bytes();
    Validator::new().validate_all(bytes).unwrap();
    let functions = Parser::new(0)
        .parse_all(bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(body),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(functions.len(), cases.len());
    for (case, function) in cases.iter().zip(functions) {
        let mut predicates = 0;
        let mut wraps = 0;
        for operator in function.get_operators_reader().unwrap() {
            match operator.unwrap() {
                Operator::I32Eqz
                | Operator::I64Eqz
                | Operator::I32Eq
                | Operator::I64Eq
                | Operator::I32Ne
                | Operator::I64Ne => predicates += 1,
                Operator::I32WrapI64 => wraps += 1,
                _ => {}
            }
        }
        assert_eq!(predicates != 0, case.needs_test, "{}", case.name);
        if !case.needs_test {
            assert_eq!(
                wraps,
                usize::from(case.parameter == Type::I64),
                "{}",
                case.name
            );
        }
    }
}

#[test]
fn zero_test_predicates_return_canonical_booleans_at_runtime() {
    let cases = cases();
    let module = module(&cases);
    let mut instance = module.instantiate();
    for case in &cases {
        for &(input, expected) in case.inputs {
            assert_eq!(
                instance.call_values(case.name, &[input]).unwrap(),
                Some(Value::I32(expected)),
                "{} with {input:?}",
                case.name,
            );
        }
    }
}
