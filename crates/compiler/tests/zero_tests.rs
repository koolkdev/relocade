#[path = "support/wasm.rs"]
mod wasm;
use wasm::ModuleFile;

use wasm86_compiler::{FunctionBuilder, Program, Signature, Type, Val, I1, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

struct Case {
    name: &'static str,
    parameter: Type,
    build: fn(&FunctionBuilder<'_>) -> Val<I1>,
    needs_test: bool,
    inputs: &'static [(&'static str, &'static str)],
}

fn cases() -> [Case; 10] {
    [
        Case {
            name: "masked32",
            parameter: Type::I32,
            build: |body| body.parameter::<I32>(0).unwrap().and(1).ne(0),
            needs_test: false,
            inputs: &[("i32:0", "0\n"), ("i32:2", "0\n"), ("i32:3", "1\n")],
        },
        Case {
            name: "shifted32",
            parameter: Type::I32,
            build: |body| body.parameter::<I32>(0).unwrap().unsigned().shr(31).ne(0),
            needs_test: false,
            inputs: &[("i32:2147483647", "0\n"), ("i32:-2147483648", "1\n")],
        },
        Case {
            name: "masked64",
            parameter: Type::I64,
            build: |body| body.parameter::<I64>(0).unwrap().and(1u64).ne(0u64),
            needs_test: false,
            inputs: &[("i64:0", "0\n"), ("i64:2", "0\n"), ("i64:3", "1\n")],
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
                ("i64:9223372036854775807", "0\n"),
                ("i64:-9223372036854775808", "1\n"),
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
            inputs: &[("i32:127", "1\n"), ("i32:255", "0\n")],
        },
        Case {
            name: "narrow_mask",
            parameter: Type::I8,
            build: |body| body.parameter::<I8>(0).unwrap().add(1).and(1).ne(0),
            needs_test: false,
            inputs: &[("i32:0", "1\n"), ("i32:1", "0\n"), ("i32:255", "0\n")],
        },
        Case {
            name: "other_bit",
            parameter: Type::I32,
            build: |body| body.parameter::<I32>(0).unwrap().and(2).ne(0),
            needs_test: true,
            inputs: &[("i32:0", "0\n"), ("i32:1", "0\n"), ("i32:2", "1\n")],
        },
        Case {
            name: "wide_nonzero",
            parameter: Type::I64,
            build: |body| body.parameter::<I64>(0).unwrap().ne(0u64),
            needs_test: true,
            inputs: &[
                ("i64:0", "0\n"),
                ("i64:2", "1\n"),
                ("i64:4294967296", "1\n"),
            ],
        },
        Case {
            name: "narrow_nonzero",
            parameter: Type::I8,
            build: |body| body.parameter::<I8>(0).unwrap().add(1).ne(0),
            needs_test: true,
            inputs: &[("i32:0", "1\n"), ("i32:1", "1\n"), ("i32:255", "0\n")],
        },
        Case {
            name: "masked_zero",
            parameter: Type::I32,
            build: |body| body.parameter::<I32>(0).unwrap().and(1).eq(0),
            needs_test: true,
            inputs: &[("i32:0", "1\n"), ("i32:1", "0\n"), ("i32:2", "1\n")],
        },
    ]
}

fn module(cases: &[Case]) -> Vec<u8> {
    let mut program = Program::new();
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
    program.compile().unwrap()
}

#[test]
fn proven_zero_or_one_values_need_no_predicate_but_keep_the_i32_result_carrier() {
    let cases = cases();
    let bytes = module(&cases);
    Validator::new().validate_all(&bytes).unwrap();
    let functions = Parser::new(0)
        .parse_all(&bytes)
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

fn check_execution(flags: &[&str]) {
    let cases = cases();
    let module = ModuleFile::new(&module(&cases));
    for case in &cases {
        for &(input, expected) in case.inputs {
            module.check(flags, "execute.mjs", &[case.name, input], expected);
        }
    }
}

#[test]
#[ignore = "requires Node.js with WebAssembly support"]
fn zero_tests_execute_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js with V8 optimizing compiler flags"]
fn zero_tests_execute_in_v8_optimizing() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
