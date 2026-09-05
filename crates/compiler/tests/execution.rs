use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use wasm86_compiler::{
    FunctionBuilder, IntType, Program, Signature, Type, Val, I1, I16, I32, I64, I8,
};

struct ModuleFile(PathBuf);

impl ModuleFile {
    fn new(program: Program) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "wasm86-scalar-{}-{}.wasm",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, program.compile().unwrap()).unwrap();
        Self(path)
    }

    fn check(&self, flags: &[&str], name: &str, args: &[&str], expected: &str) {
        let output = Command::new("node")
            .args(flags)
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/execute.mjs"
            ))
            .arg(&self.0)
            .arg(name)
            .args(args)
            .output()
            .expect("the explicit V8 lane requires Node.js on PATH");
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            expected,
            "{name}({args:?}), V8 flags {flags:?}"
        );
    }
}

impl Drop for ModuleFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn define_export<T: IntType>(
    program: &mut Program,
    name: &str,
    parameters: &[Type],
    build: impl FnOnce(&FunctionBuilder<'_>) -> Val<T>,
) {
    let function = program.declare(Signature {
        parameters: parameters.to_vec(),
        result: T::TYPE,
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
        body.constant::<T>(bits)
    });
    define_export(program, &format!("negative{suffix}"), &[], |body| {
        body.constant::<T>(-1)
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
        define_export(&mut program, name, &[], |body| body.constant::<I32>(bits));
    }
    for (name, bits) in [
        ("zero64", 0_u64),
        ("min64", 0x8000_0000_0000_0000),
        ("max64", 0x7fff_ffff_ffff_ffff),
        ("all64", u64::MAX),
    ] {
        define_export(&mut program, name, &[], |body| body.constant::<I64>(bits));
    }
    define_export(&mut program, "signed_literal32", &[], |body| {
        body.constant::<I32>(-2147483647)
    });
    define_export(&mut program, "signed_literal64", &[], |body| {
        body.constant::<I64>(0).add(-1)
    });
    define_export(&mut program, "unsigned_literal64", &[], |body| {
        body.constant::<I64>(0).add(u32::MAX)
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
        body.constant::<I32>(0x7fff_ffff).add(1)
    });
    define_export(&mut program, "constant_add64", &[], |body| {
        body.constant::<I64>(u64::MAX).add(1)
    });

    let signature = Signature {
        parameters: vec![],
        result: Type::I32,
    };
    let first = program.declare(signature.clone());
    let second = program.declare(signature);
    for (function, value) in [(second, 11), (first, 7)] {
        let body = program.define(function).unwrap();
        let value = body.constant::<I32>(value);
        body.return_(&value).unwrap();
    }
    program.export("first", first).unwrap();
    program.export("second", second).unwrap();
    program.export("second_alias", second).unwrap();

    define_narrow_functions::<I1>(&mut program, "1", 2);
    define_narrow_functions::<I8>(&mut program, "8", 0x180);
    define_narrow_functions::<I16>(&mut program, "16", 0x18000);
    define_export(&mut program, "boolean_add", &[], |body| {
        body.constant::<I1>(true).add(true)
    });

    let module = ModuleFile::new(program);
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
        module.check(flags, name, &[], expected);
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
        module.check(flags, name, &[], expected);
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
        module.check(flags, name, &[arg], expected);
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
        module.check(flags, name, &args, expected);
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
        module.check(flags, name, &args, expected);
    }
    for (name, args) in [
        ("overlap32", ["i32:4", "i32:10"]),
        ("disjoint32", ["i32:4", "i32:10"]),
        ("overlap64", ["i64:4", "i64:10"]),
        ("disjoint64", ["i64:4", "i64:10"]),
    ] {
        module.check(flags, name, &args, "34");
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
        module.check(flags, name, &args, expected);
    }
    for (name, arg, expected) in [
        ("shared32", "i32:4", "14"),
        ("shared32", "i32:2147483647", "4"),
        ("shared64", "i64:4", "14"),
        ("shared64", "i64:9223372036854775807", "4"),
    ] {
        module.check(flags, name, &[arg], expected);
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
