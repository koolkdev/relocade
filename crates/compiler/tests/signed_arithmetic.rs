#[path = "support/wasm.rs"]
mod wasm;
use wasm::ModuleFile;

use wasm86_compiler::{
    FunctionBuilder, IntType, MemoryImport, Program, Signature, Type, Val, I1, I16, I32, I64, I8,
};
use wasmparser::{Operator, Parser, Payload, Validator};

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
                result: T::TYPE,
            },
            |body| {
                let result = build(&body);
                body.return_(result)
            },
        )
        .unwrap();
    program.export(name, function).unwrap();
}

fn module<T: IntType>(
    parameters: &[Type],
    build: impl FnOnce(&FunctionBuilder<'_>) -> Val<T>,
) -> Vec<u8> {
    let mut program = Program::new();
    function(&mut program, "run", parameters, build);
    program.compile().unwrap()
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

fn operations() -> Vec<u8> {
    let mut program = Program::new();
    arithmetic::<I1>(&mut program, "1");
    arithmetic::<I8>(&mut program, "8");
    arithmetic::<I16>(&mut program, "16");
    arithmetic::<I32>(&mut program, "32");
    arithmetic::<I64>(&mut program, "64");
    program.compile().unwrap()
}

fn shared_underflow() -> Vec<u8> {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "state".into(),
        minimum: 1,
        maximum: None,
    });
    let run = program
        .function(
            Signature {
                parameters: vec![],
                result: Type::I32,
            },
            |mut body| {
                let difference = body.load::<I8>(memory, 0)?.sub(1);
                body.store(memory, 0, &difference)?;
                let negative = difference.signed().lt(0).unsigned().extend::<I32>();
                body.return_(difference.unsigned().extend::<I32>().add(negative.shl(8)))
            },
        )
        .unwrap();
    program.export("run", run).unwrap();
    program.compile().unwrap()
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
fn wrapping_difference_uses_one_subtraction_and_signed_carrier_comparison() {
    let bytes = module(&[Type::I32; 2], |body| {
        let left = body.parameter::<I32>(0).unwrap();
        let right = body.parameter::<I32>(1).unwrap();
        left.sub(right).signed().ge(0)
    });
    assert!(matches!(
        operators(&bytes).as_slice(),
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
fn underflow_shares_raw_store_bits_with_signed_and_unsigned_observers() {
    let bytes = shared_underflow();
    let operations = operators(&bytes);
    let relevant: Vec<_> = operations
        .iter()
        .filter_map(|operator| match operator {
            Operator::I32Load8U { .. } => Some("read"),
            Operator::I32Sub => Some("subtract"),
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
            "subtract",
            "store",
            "unsigned low bits",
            "signed low bits"
        ]
    );
}

#[test]
fn constant_subtraction_and_comparisons_fold_at_the_logical_width() {
    for (bytes, expected) in [
        (
            module(&[], |b| b.value::<I8>(0).unwrap().sub(1).signed().lt(0)),
            1,
        ),
        (
            module(&[], |b| b.value::<I8>(0).unwrap().sub(129).signed().lt(0)),
            0,
        ),
        (
            module(&[], |b| {
                b.value::<I16>(0).unwrap().sub(32769).signed().ge(0)
            }),
            1,
        ),
        (
            module(&[], |b| {
                b.value::<I32>(0x8000_0000u32)
                    .unwrap()
                    .sub(1)
                    .signed()
                    .ge(0)
            }),
            1,
        ),
        (
            module(&[], |b| b.value::<I64>(0).unwrap().sub(1).signed().ge(0)),
            0,
        ),
        (
            module(&[], |b| {
                b.value::<I1>(false).unwrap().sub(true).signed().lt(false)
            }),
            1,
        ),
        (
            module(&[Type::I32], |b| {
                let value = b.parameter::<I32>(0).unwrap();
                value.sub(&value).signed().ge(0)
            }),
            1,
        ),
    ] {
        assert!(matches!(
            operators(&bytes).as_slice(),
            [Operator::I32Const { value }, Operator::Return, Operator::End] if *value == expected
        ));
    }
}

fn check_execution(flags: &[&str]) {
    let bytes = operations();
    Validator::new().validate_all(&bytes).unwrap();
    let module = ModuleFile::new(&bytes);
    for (name, left, right, expected) in [
        ("sub1", "i32:0", "i32:1", "1\n"),
        ("sub8", "i32:0", "i32:1", "255\n"),
        ("sub16", "i32:0", "i32:1", "65535\n"),
        ("sub32", "i32:-2147483648", "i32:1", "2147483647\n"),
        ("sub32", "i32:2147483647", "i32:-1", "-2147483648\n"),
        (
            "sub64",
            "i64:-9223372036854775808",
            "i64:1",
            "9223372036854775807\n",
        ),
        (
            "sub64",
            "i64:9223372036854775807",
            "i64:-1",
            "-9223372036854775808\n",
        ),
        ("lt1", "i32:1", "i32:0", "1\n"),
        ("lt1", "i32:0", "i32:1", "0\n"),
        ("lt8", "i32:128", "i32:127", "1\n"),
        ("lt8", "i32:127", "i32:255", "0\n"),
        ("lt16", "i32:32768", "i32:32767", "1\n"),
        ("lt32", "i32:-2147483648", "i32:2147483647", "1\n"),
        (
            "lt64",
            "i64:-9223372036854775808",
            "i64:9223372036854775807",
            "1\n",
        ),
        ("difference_nonnegative1", "i32:0", "i32:1", "0\n"),
        ("difference_nonnegative1", "i32:1", "i32:1", "1\n"),
        ("difference_nonnegative8", "i32:0", "i32:1", "0\n"),
        ("difference_nonnegative8", "i32:0", "i32:129", "1\n"),
        ("difference_nonnegative8", "i32:128", "i32:1", "1\n"),
        ("difference_nonnegative16", "i32:0", "i32:32769", "1\n"),
        ("difference_nonnegative16", "i32:32768", "i32:1", "1\n"),
        ("difference_nonnegative32", "i32:-1", "i32:1", "0\n"),
        ("difference_nonnegative32", "i32:0", "i32:-1", "1\n"),
        ("difference_nonnegative32", "i32:7", "i32:7", "1\n"),
        ("difference_nonnegative32", "i32:7", "i32:8", "0\n"),
        (
            "difference_nonnegative32",
            "i32:-2147483648",
            "i32:1",
            "1\n",
        ),
        ("difference_nonnegative64", "i64:0", "i64:1", "0\n"),
        (
            "difference_nonnegative64",
            "i64:-9223372036854775808",
            "i64:1",
            "1\n",
        ),
    ] {
        module.check(flags, "execute.mjs", &[name, left, right], expected);
    }
    let module = ModuleFile::new(&shared_underflow());
    module.check(
        flags,
        "execute-memory.mjs",
        &["state:00a5"],
        "511\nstate:ffa5\n",
    );
    module.check(
        flags,
        "execute-memory.mjs",
        &["state:07a5"],
        "6\nstate:06a5\n",
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn signed_arithmetic_executes_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn signed_arithmetic_executes_in_v8_optimizing() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
