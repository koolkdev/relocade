#[path = "support/wasm.rs"]
mod wasm;
use wasm::ModuleFile;

use wasm86_compiler::{
    FunctionBuilder, IntType, Program, Signature, Type, Val, I1, I16, I32, I64, I8,
};

fn check(module: &ModuleFile, flags: &[&str], name: &str, args: &[&str], expected: &str) {
    let arguments = [&[name], args].concat();
    module.check(flags, "execute.mjs", &arguments, &format!("{expected}\n"));
}

fn define_export<T: IntType>(
    program: &mut Program,
    name: &str,
    parameters: &[Type],
    build: impl FnOnce(&FunctionBuilder<'_>) -> Val<T>,
) {
    let function = program.declare(Signature {
        parameters: parameters.to_vec(),
        result: Some(T::TYPE),
    });
    let body = program.define(function).unwrap();
    let result = build(&body);
    body.return_(&result).unwrap();
    program.export(name, function).unwrap();
}

fn define_arithmetic_functions<T: IntType>(program: &mut Program, suffix: &str) {
    let ty = T::TYPE;
    define_export(program, &format!("add{suffix}"), &[ty, ty], |body| {
        let left = body.parameter::<T>(0).unwrap();
        let right = body.parameter::<T>(1).unwrap();
        left.add(&right)
    });
    define_export(program, &format!("shared{suffix}"), &[ty], |body| {
        let value = body.parameter::<T>(0).unwrap();
        let child = value.add(1);
        let shared = child.add(2);
        shared.add(&shared)
    });
    for (name, overlap) in [("overlap", true), ("disjoint", false)] {
        define_export(program, &format!("{name}{suffix}"), &[ty, ty], |body| {
            let first = body.parameter::<T>(0).unwrap();
            let second = body.parameter::<T>(1).unwrap();
            let a = first.add(1);
            let b = second.add(2);
            let _dead = a.add(9);
            let (left, right) = if overlap {
                (a.add(&b), b.add(&a))
            } else {
                (a.add(&a), b.add(&b))
            };
            left.add(&right)
        });
    }
}

fn define_narrow_functions<T: IntType>(program: &mut Program, suffix: &str, bits: u32) {
    let ty = T::TYPE;
    define_export(program, &format!("literal{suffix}"), &[], |body| {
        body.value::<T>(bits).unwrap()
    });
    define_export(program, &format!("negative{suffix}"), &[], |body| {
        body.value::<T>(-1).unwrap()
    });
    define_export(program, &format!("identity{suffix}"), &[ty], |body| {
        body.parameter::<T>(0).unwrap()
    });
    define_export(program, &format!("add{suffix}"), &[ty, ty], |body| {
        let left = body.parameter::<T>(0).unwrap();
        let right = body.parameter::<T>(1).unwrap();
        left.add(&right)
    });
    define_export(program, &format!("shared{suffix}"), &[ty], |body| {
        let a = body.parameter::<T>(0).unwrap().add(1);
        let b = a.add(1);
        b.add(&b)
    });
}

fn check_execution(flags: &[&str]) {
    let mut program = Program::new();
    for (name, bits) in [
        ("zero32", 0_u32),
        ("min32", 0x8000_0000),
        ("max32", 0x7fff_ffff),
        ("all32", u32::MAX),
    ] {
        define_export(&mut program, name, &[], |body| {
            body.value::<I32>(bits).unwrap()
        });
    }
    for (name, bits) in [
        ("zero64", 0_u64),
        ("min64", 0x8000_0000_0000_0000),
        ("max64", 0x7fff_ffff_ffff_ffff),
        ("all64", u64::MAX),
    ] {
        define_export(&mut program, name, &[], |body| {
            body.value::<I64>(bits).unwrap()
        });
    }
    define_export(&mut program, "signed_literal32", &[], |body| {
        body.value::<I32>(-2147483647).unwrap()
    });
    define_export(&mut program, "signed_literal64", &[], |body| {
        body.value::<I64>(0).unwrap().add(-1)
    });
    define_export(&mut program, "unsigned_literal64", &[], |body| {
        body.value::<I64>(0).unwrap().add(u32::MAX)
    });
    define_arithmetic_functions::<I32>(&mut program, "32");
    define_arithmetic_functions::<I64>(&mut program, "64");

    let parameters = [Type::I32, Type::I64, Type::I32, Type::I64];
    for index in [0, 2] {
        define_export(
            &mut program,
            &format!("param{index}"),
            &parameters,
            |body| body.parameter::<I32>(index).unwrap(),
        );
    }
    for index in [1, 3] {
        define_export(
            &mut program,
            &format!("param{index}"),
            &parameters,
            |body| body.parameter::<I64>(index).unwrap(),
        );
    }
    define_export(&mut program, "constant_add32", &[], |body| {
        body.value::<I32>(0x7fff_ffff).unwrap().add(1)
    });
    define_export(&mut program, "constant_add64", &[], |body| {
        body.value::<I64>(u64::MAX).unwrap().add(1)
    });

    let signature = Signature {
        parameters: vec![],
        result: Some(Type::I32),
    };
    let first = program.declare(signature.clone());
    let second = program.declare(signature);
    for (function, value) in [(second, 11), (first, 7)] {
        let body = program.define(function).unwrap();
        body.return_(value).unwrap();
    }
    program.export("first", first).unwrap();
    program.export("second", second).unwrap();
    program.export("second_alias", second).unwrap();

    define_narrow_functions::<I1>(&mut program, "1", 2);
    define_narrow_functions::<I8>(&mut program, "8", 0x180);
    define_narrow_functions::<I16>(&mut program, "16", 0x18000);
    define_export(&mut program, "boolean_add", &[], |body| {
        body.value::<I1>(true).unwrap().add(true)
    });

    let module = ModuleFile::new(&program.compile().unwrap());
    for (name, expected) in [
        ("zero32", "0"),
        ("min32", "-2147483648"),
        ("max32", "2147483647"),
        ("all32", "-1"),
        ("signed_literal32", "-2147483647"),
        ("signed_literal64", "-1"),
        ("unsigned_literal64", "4294967295"),
        ("zero64", "0"),
        ("min64", "-9223372036854775808"),
        ("max64", "9223372036854775807"),
        ("all64", "-1"),
        ("constant_add32", "-2147483648"),
        ("constant_add64", "0"),
        ("first", "7"),
        ("second", "11"),
        ("second_alias", "11"),
    ] {
        check(&module, flags, name, &[], expected);
    }
    for (name, expected) in [
        ("literal1", "0"),
        ("negative1", "1"),
        ("literal8", "128"),
        ("negative8", "255"),
        ("literal16", "32768"),
        ("negative16", "65535"),
        ("boolean_add", "0"),
    ] {
        check(&module, flags, name, &[], expected);
    }
    for (name, arg, expected) in [
        ("identity1", "i32:0", "0"),
        ("identity1", "i32:1", "1"),
        ("identity8", "i32:255", "255"),
        ("identity16", "i32:65535", "65535"),
        ("shared1", "i32:1", "0"),
        ("shared8", "i32:254", "0"),
        ("shared8", "i32:255", "2"),
        ("shared16", "i32:65534", "0"),
        ("shared16", "i32:65535", "2"),
    ] {
        check(&module, flags, name, &[arg], expected);
    }
    for (name, args, expected) in [
        ("add1", ["i32:0", "i32:1"], "1"),
        ("add1", ["i32:1", "i32:1"], "0"),
        ("add8", ["i32:255", "i32:1"], "0"),
        ("add8", ["i32:127", "i32:1"], "128"),
        ("add8", ["i32:255", "i32:255"], "254"),
        ("add16", ["i32:65535", "i32:1"], "0"),
        ("add16", ["i32:32767", "i32:1"], "32768"),
        ("add16", ["i32:65535", "i32:65535"], "65534"),
    ] {
        check(&module, flags, name, &args, expected);
    }
    for (name, args, expected) in [
        ("add32", ["i32:2147483647", "i32:1"], "-2147483648"),
        ("add32", ["i32:-2147483648", "i32:-1"], "2147483647"),
        ("add32", ["i32:-1", "i32:1"], "0"),
        ("add32", ["i32:19", "i32:-7"], "12"),
        (
            "add64",
            ["i64:9223372036854775807", "i64:1"],
            "-9223372036854775808",
        ),
        (
            "add64",
            ["i64:-9223372036854775808", "i64:-1"],
            "9223372036854775807",
        ),
        ("add64", ["i64:-1", "i64:1"], "0"),
        ("add64", ["i64:19", "i64:-7"], "12"),
    ] {
        check(&module, flags, name, &args, expected);
    }
    for (name, args) in [
        ("overlap32", ["i32:4", "i32:10"]),
        ("disjoint32", ["i32:4", "i32:10"]),
        ("overlap64", ["i64:4", "i64:10"]),
        ("disjoint64", ["i64:4", "i64:10"]),
    ] {
        check(&module, flags, name, &args, "34");
    }
    let args = [
        "i32:17",
        "i64:-9223372036854775808",
        "i32:-91",
        "i64:9223372036854775807",
    ];
    for (name, expected) in [
        ("param0", "17"),
        ("param1", "-9223372036854775808"),
        ("param2", "-91"),
        ("param3", "9223372036854775807"),
    ] {
        check(&module, flags, name, &args, expected);
    }
    for (name, arg, expected) in [
        ("shared32", "i32:4", "14"),
        ("shared32", "i32:2147483647", "4"),
        ("shared64", "i64:4", "14"),
        ("shared64", "i64:9223372036854775807", "4"),
    ] {
        check(&module, flags, name, &[arg], expected);
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn scalar_values_execute_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn scalar_values_execute_in_v8_optimizing() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
