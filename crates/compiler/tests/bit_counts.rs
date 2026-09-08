#[path = "support/wasm.rs"]
mod wasm;
use wasm::ModuleFile;
use wasm86_compiler::{IntType, Program, Signature, Type, I1, I16, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

fn population<T: IntType>(program: &mut Program, name: &str) {
    let function = program
        .function(
            Signature {
                parameters: vec![T::TYPE],
                result: T::TYPE,
            },
            |body| {
                let input = body.parameter::<T>(0)?;
                body.return_(input.add(1).popcnt())
            },
        )
        .unwrap();
    program.export(name, function).unwrap();
}

fn operations() -> Vec<u8> {
    let mut program = Program::new();
    population::<I1>(&mut program, "bits1");
    population::<I8>(&mut program, "bits8");
    population::<I16>(&mut program, "bits16");
    population::<I32>(&mut program, "bits32");
    population::<I64>(&mut program, "bits64");
    let xor = program
        .function(
            Signature {
                parameters: vec![Type::I32; 2],
                result: Type::I32,
            },
            |body| {
                let left = body.parameter::<I32>(0)?;
                let right = body.parameter::<I32>(1)?;
                body.return_(left.xor(right))
            },
        )
        .unwrap();
    program.export("xor", xor).unwrap();
    program.compile().unwrap()
}

#[test]
fn population_counts_use_logical_input_bits_and_native_carrier_operations() {
    let bytes = operations();
    Validator::new().validate_all(&bytes).unwrap();
    let mut masks = 0;
    let mut counts32 = 0;
    let mut counts64 = 0;
    let mut xors = 0;
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for op in body.get_operators_reader().unwrap() {
                match op.unwrap() {
                    Operator::I32And => masks += 1,
                    Operator::I32Popcnt => counts32 += 1,
                    Operator::I64Popcnt => counts64 += 1,
                    Operator::I32Xor => xors += 1,
                    _ => {}
                }
            }
        }
    }
    assert_eq!((masks, counts32, counts64, xors), (3, 4, 1, 1));
}

#[test]
fn constant_xor_and_population_count_fold_without_runtime_work() {
    let mut program = Program::new();
    program
        .function(
            Signature {
                parameters: vec![],
                result: Type::I64,
            },
            |body| {
                let count = body
                    .value::<I64>(0xff00_ff00_ff00_ff00u64)?
                    .xor(0x0f0f_0f0f_0f0f_0f0fu64)
                    .popcnt();
                body.return_(count)
            },
        )
        .unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let operators = body
                .get_operators_reader()
                .unwrap()
                .into_iter()
                .map(Result::unwrap)
                .collect::<Vec<_>>();
            assert!(matches!(
                operators.as_slice(),
                [
                    Operator::I64Const { value: 32 },
                    Operator::Return,
                    Operator::End
                ]
            ));
        }
    }
}

fn check(flags: &[&str]) {
    let module = ModuleFile::new(&operations());
    for (name, argument, expected) in [
        ("bits1", "i32:0", "1\n"),
        ("bits1", "i32:1", "0\n"),
        ("bits8", "i32:254", "8\n"),
        ("bits8", "i32:255", "0\n"),
        ("bits16", "i32:65534", "16\n"),
        ("bits16", "i32:65535", "0\n"),
        ("bits32", "i32:-2", "32\n"),
        ("bits32", "i32:-1", "0\n"),
        ("bits64", "i64:-2", "64\n"),
        ("bits64", "i64:-1", "0\n"),
    ] {
        module.check(flags, "execute.mjs", &[name, argument], expected);
    }
    module.check(
        flags,
        "execute.mjs",
        &["xor", "i32:-1", "i32:2147483647"],
        "-2147483648\n",
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn bit_counts_execute_in_v8() {
    check(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn bit_counts_execute_in_v8_optimizing() {
    check(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
