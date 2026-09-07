use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use wasm86_compiler::{
    AtLeast, FunctionBuilder, FunctionImport, IntType, Mem, MemoryImport, Program, Signature, Type,
    Val, I1, I16, I32, I64, I8,
};
use wasmparser::{Operator, Parser, Payload, Validator};

fn function<T: IntType>(
    program: &mut Program,
    name: &str,
    parameters: &[Type],
    build: impl FnOnce(&FunctionBuilder<'_>) -> Val<T>,
) {
    let declared = program.declare(Signature {
        parameters: parameters.to_vec(),
        result: T::TYPE,
    });
    let body = program.define(declared).unwrap();
    let value = build(&body);
    body.return_(&value).unwrap();
    program.export(name, declared).unwrap();
}

fn module<T: IntType>(
    parameters: &[Type],
    build: impl FnOnce(&FunctionBuilder<'_>) -> Val<T>,
) -> Vec<u8> {
    let mut program = Program::new();
    function(&mut program, "run", parameters, build);
    program.compile().unwrap()
}

fn narrow_functions<T: IntType>(program: &mut Program, suffix: &str)
where
    I32: AtLeast<T>,
{
    let parameters = &[T::TYPE];
    function(program, &format!("shr{suffix}"), parameters, |b| {
        b.parameter::<T>(0).unwrap().add(1).unsigned().shr(1)
    });
    function(program, &format!("eqzero{suffix}"), parameters, |b| {
        b.parameter::<T>(0).unwrap().add(1).eq(0)
    });
    function(program, &format!("nezero{suffix}"), parameters, |b| {
        b.parameter::<T>(0).unwrap().add(1).ne(0)
    });
    function(program, &format!("lt{suffix}"), parameters, |b| {
        b.parameter::<T>(0).unwrap().add(1).unsigned().lt(1)
    });
    function(program, &format!("ge{suffix}"), parameters, |b| {
        b.parameter::<T>(0).unwrap().add(1).unsigned().ge(1)
    });
    function(program, &format!("extend{suffix}"), parameters, |b| {
        b.parameter::<T>(0)
            .unwrap()
            .add(1)
            .unsigned()
            .extend::<I32>()
    });
}

fn operations() -> Vec<u8> {
    let mut p = Program::new();
    function(&mut p, "bits32", &[Type::I32], |b| {
        b.parameter::<I32>(0)
            .unwrap()
            .and(0x00ff_00ffu32)
            .or(0x8000_0000u32)
    });
    function(&mut p, "bits64", &[Type::I64], |b| {
        b.parameter::<I64>(0)
            .unwrap()
            .and(0x8000_0000_0000_0000u64)
            .or(1)
    });
    for count in [31, 33] {
        function(&mut p, &format!("shl32_{count}"), &[Type::I32], |b| {
            b.parameter::<I32>(0).unwrap().shl(count)
        });
    }
    for count in [31, 32] {
        function(&mut p, &format!("shr32_{count}"), &[Type::I32], |b| {
            b.parameter::<I32>(0).unwrap().unsigned().shr(count)
        });
    }
    for count in [63, 65] {
        function(&mut p, &format!("shl64_{count}"), &[Type::I64], |b| {
            b.parameter::<I64>(0).unwrap().shl(count)
        });
    }
    for count in [63, 64] {
        function(&mut p, &format!("shr64_{count}"), &[Type::I64], |b| {
            b.parameter::<I64>(0).unwrap().unsigned().shr(count)
        });
    }
    function(&mut p, "eq32", &[Type::I32; 2], |b| {
        b.parameter::<I32>(0)
            .unwrap()
            .eq(b.parameter::<I32>(1).unwrap())
    });
    function(&mut p, "ne32", &[Type::I32; 2], |b| {
        b.parameter::<I32>(0)
            .unwrap()
            .ne(b.parameter::<I32>(1).unwrap())
    });
    function(&mut p, "lt32", &[Type::I32; 2], |b| {
        b.parameter::<I32>(0)
            .unwrap()
            .unsigned()
            .lt(b.parameter::<I32>(1).unwrap())
    });
    function(&mut p, "ge32", &[Type::I32; 2], |b| {
        b.parameter::<I32>(0)
            .unwrap()
            .unsigned()
            .ge(b.parameter::<I32>(1).unwrap())
    });
    function(&mut p, "eq64", &[Type::I64; 2], |b| {
        b.parameter::<I64>(0)
            .unwrap()
            .eq(b.parameter::<I64>(1).unwrap())
    });
    function(&mut p, "lt64", &[Type::I64; 2], |b| {
        b.parameter::<I64>(0)
            .unwrap()
            .unsigned()
            .lt(b.parameter::<I64>(1).unwrap())
    });
    function(&mut p, "ge64", &[Type::I64; 2], |b| {
        b.parameter::<I64>(0)
            .unwrap()
            .unsigned()
            .ge(b.parameter::<I64>(1).unwrap())
    });
    function(&mut p, "truncate64", &[Type::I64], |b| {
        b.parameter::<I64>(0).unwrap().truncate::<I32>()
    });
    function(&mut p, "truncate8", &[Type::I32], |b| {
        b.parameter::<I32>(0).unwrap().truncate::<I8>()
    });
    function(&mut p, "truncate1", &[Type::I32], |b| {
        b.parameter::<I32>(0).unwrap().truncate::<I1>()
    });
    function(&mut p, "extend32", &[Type::I32], |b| {
        b.parameter::<I32>(0).unwrap().unsigned().extend::<I64>()
    });
    function(&mut p, "shr8_8", &[Type::I8], |b| {
        b.parameter::<I8>(0).unwrap().unsigned().shr(8)
    });
    function(&mut p, "shr8_32", &[Type::I8], |b| {
        b.parameter::<I8>(0).unwrap().unsigned().shr(32)
    });
    function(&mut p, "opcode_group", &[Type::I8], |b| {
        b.parameter::<I8>(0).unwrap().and(0xf8).eq(0xb8)
    });
    function(&mut p, "opcode_register", &[Type::I8], |b| {
        b.parameter::<I8>(0).unwrap().and(7)
    });
    function(&mut p, "assemble", &[Type::I8; 4], |b| {
        let byte0 = b.parameter::<I8>(0).unwrap().unsigned().extend::<I32>();
        let byte1 = b
            .parameter::<I8>(1)
            .unwrap()
            .unsigned()
            .extend::<I32>()
            .shl(8);
        let byte2 = b
            .parameter::<I8>(2)
            .unwrap()
            .unsigned()
            .extend::<I32>()
            .shl(16);
        let byte3 = b
            .parameter::<I8>(3)
            .unwrap()
            .unsigned()
            .extend::<I32>()
            .shl(24);
        byte0.or(&byte1).or(&byte2).or(&byte3)
    });
    function(&mut p, "shr_wrap_truncate", &[Type::I32], |b| {
        b.parameter::<I32>(0)
            .unwrap()
            .unsigned()
            .shr(33)
            .truncate::<I8>()
    });
    for count in [1, 8, 32] {
        function(&mut p, &format!("shl8_{count}"), &[Type::I8], |b| {
            b.parameter::<I8>(0).unwrap().shl(count)
        });
    }
    narrow_functions::<I1>(&mut p, "1");
    narrow_functions::<I8>(&mut p, "8");
    narrow_functions::<I16>(&mut p, "16");
    p.compile().unwrap()
}

fn narrow_comparison(ne: bool, zero: bool) -> Vec<u8> {
    let parameters = if zero {
        vec![Type::I8]
    } else {
        vec![Type::I8; 2]
    };
    module(&parameters, |b| {
        let a = b.parameter::<I8>(0).unwrap().add(1);
        let other = if zero {
            let _unused = a.unsigned().extend::<I32>();
            b.value::<I8>(0).unwrap()
        } else {
            b.parameter::<I8>(1).unwrap().add(3)
        };
        if ne {
            a.ne(&other)
        } else {
            a.eq(&other)
        }
    })
}

fn shared_conversion() -> Vec<u8> {
    module(&[Type::I8], |b| {
        let byte = b.parameter::<I8>(0).unwrap().and(7);
        let wide = byte.unsigned().extend::<I16>().unsigned().extend::<I32>();
        wide.add(&wide)
    })
}

fn conversion_roundtrip() -> Vec<u8> {
    module(&[Type::I32], |b| {
        let masked = b.parameter::<I32>(0).unwrap().and(255);
        let wide = masked.truncate::<I8>().unsigned().extend::<I32>();
        wide.add(&wide)
    })
}

fn state(program: &mut Program) -> Mem {
    program.import_memory(MemoryImport {
        module: "test".into(),
        name: "state".into(),
        minimum: 1,
        maximum: None,
    })
}

fn load_conversion() -> Vec<u8> {
    let mut p = Program::new();
    let memory = state(&mut p);
    let run = p.declare(Signature {
        parameters: vec![],
        result: Type::I32,
    });
    let mut b = p.define(run).unwrap();
    let loaded = b.load::<I8>(memory, 0).unwrap();
    let wide = loaded.unsigned().extend::<I32>();
    let masked = wide.and(7);
    b.store::<I8>(memory, 0, 0).unwrap();
    b.return_(wide.add(&masked)).unwrap();
    p.export("run", run).unwrap();
    p.compile().unwrap()
}

fn shared_boundary() -> Vec<u8> {
    let mut p = Program::new();
    let memory = state(&mut p);
    let target = p.import_function(FunctionImport {
        module: "test".into(),
        name: "receive".into(),
        signature: Signature {
            parameters: vec![Type::I32, Type::I8, Type::I1, Type::I32],
            result: Type::I64,
        },
    });
    let run = p.declare(Signature {
        parameters: vec![Type::I8],
        result: Type::I64,
    });
    let mut b = p.define(run).unwrap();
    let raw = b.parameter::<I8>(0).unwrap().add(1);
    b.store(memory, 0, &raw).unwrap();
    let wide = raw.unsigned().extend::<I32>();
    let shifted = raw.unsigned().shr(1);
    let zero = raw.eq(0);
    b.tail_call(
        target,
        &[
            wide.argument(),
            shifted.argument(),
            zero.argument(),
            wide.argument(),
        ],
    )
    .unwrap();
    p.export("run", run).unwrap();
    p.compile().unwrap()
}

#[derive(Default, Debug)]
struct Code {
    adds: usize,
    masks: usize,
    shifts: usize,
    comparisons: usize,
    conversions: usize,
    locals: u32,
    writes: usize,
    accesses: Vec<&'static str>,
    constants: Vec<i32>,
    returns: usize,
}
fn inspect(bytes: &[u8]) -> Code {
    Validator::new().validate_all(bytes).unwrap();
    let mut code = Code::default();
    let mut bodies = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            bodies += 1;
            for local in body.get_locals_reader().unwrap() {
                code.locals += local.unwrap().0;
            }
            let mut ops = body.get_operators_reader().unwrap();
            while !ops.eof() {
                match ops.read().unwrap() {
                    Operator::I32Const { value } => code.constants.push(value),
                    Operator::Return => code.returns += 1,
                    Operator::I32Add | Operator::I64Add => code.adds += 1,
                    Operator::I32And | Operator::I64And => code.masks += 1,
                    Operator::I32Shl | Operator::I32ShrU | Operator::I64Shl | Operator::I64ShrU => {
                        code.shifts += 1
                    }
                    Operator::I32Eq
                    | Operator::I32Ne
                    | Operator::I32Eqz
                    | Operator::I64Eq
                    | Operator::I64Ne
                    | Operator::I64Eqz => code.comparisons += 1,
                    Operator::I32WrapI64 | Operator::I64ExtendI32U => code.conversions += 1,
                    Operator::LocalSet { .. } | Operator::LocalTee { .. } => code.writes += 1,
                    Operator::I32Load8U { .. } => code.accesses.push("load"),
                    Operator::I32Store8 { .. } => code.accesses.push("store"),
                    _ => {}
                }
            }
        }
    }
    assert_eq!(bodies, 1);
    code
}

#[test]
fn extraction_and_predicates_share_their_computed_values() {
    let bytes = module(&[Type::I32], |b| {
        let field = b.parameter::<I32>(0).unwrap().unsigned().shr(8).and(255);
        let _unused = field.or(7);
        field.add(&field)
    });
    let code = inspect(&bytes);
    assert_eq!(
        (code.shifts, code.masks, code.adds, code.locals, code.writes),
        (1, 1, 1, 1, 1)
    );
    let bytes = module(&[Type::I32; 2], |b| {
        let predicate = b
            .parameter::<I32>(0)
            .unwrap()
            .eq(b.parameter::<I32>(1).unwrap());
        let widened = predicate.unsigned().extend::<I32>();
        widened.add(&widened)
    });
    let code = inspect(&bytes);
    assert_eq!(
        (
            code.comparisons,
            code.masks,
            code.adds,
            code.locals,
            code.writes
        ),
        (1, 0, 1, 1, 1)
    );
}

#[test]
fn conversions_preserve_sharing_without_redundant_masks_or_locals() {
    for bytes in [shared_conversion(), conversion_roundtrip()] {
        let code = inspect(&bytes);
        assert_eq!(
            (
                code.masks,
                code.conversions,
                code.adds,
                code.locals,
                code.writes
            ),
            (1, 0, 1, 1, 1)
        );
    }
    let bytes = module(&[Type::I32], |b| {
        b.parameter::<I32>(0)
            .unwrap()
            .truncate::<I32>()
            .unsigned()
            .extend::<I32>()
    });
    let code = inspect(&bytes);
    assert_eq!(
        (code.masks, code.conversions, code.locals, code.writes),
        (0, 0, 0, 0)
    );
}

#[test]
fn converted_loads_preserve_the_snapshot_across_overlapping_stores() {
    let code = inspect(&load_conversion());
    assert_eq!(code.accesses, ["load", "store"]);
    assert_eq!(
        (code.masks, code.adds, code.locals, code.writes),
        (1, 1, 1, 1)
    );
}

#[test]
fn narrow_observers_share_normalization_after_a_raw_store() {
    let code = inspect(&shared_boundary());
    assert_eq!(code.accesses, ["store"]);
    assert_eq!(
        (
            code.masks,
            code.shifts,
            code.comparisons,
            code.adds,
            code.locals,
            code.writes
        ),
        (1, 1, 1, 1, 1, 2)
    );
}

#[test]
fn constant_integer_operations_fold_before_emission() {
    let bytes = module(&[], |b| {
        b.value::<I64>(0xffff_ffff_1234_5678u64)
            .unwrap()
            .and(0xffff_ffffu64)
            .or(3)
            .shl(65)
            .unsigned()
            .shr(1)
            .truncate::<I32>()
            .eq(0x1234_567b)
    });
    let code = inspect(&bytes);
    assert_eq!(code.constants, [1]);
    assert_eq!(code.returns, 1);
    assert_eq!(
        (
            code.adds,
            code.masks,
            code.shifts,
            code.comparisons,
            code.conversions,
            code.locals
        ),
        (0, 0, 0, 0, 0, 0)
    );
}

struct ModuleFile(PathBuf);
impl ModuleFile {
    fn new(bytes: &[u8]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "wasm86-integer-{}-{}.wasm",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, bytes).unwrap();
        Self(path)
    }
    fn check(&self, flags: &[&str], adapter: &str, args: &[&str], expected: &str) {
        let output = Command::new("node")
            .args(flags)
            .arg(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/support")
                    .join(adapter),
            )
            .arg(&self.0)
            .args(args)
            .output()
            .expect("the explicit V8 lane requires Node.js on PATH");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected,
            "args {args:?}, V8 flags {flags:?}"
        );
    }
}
impl Drop for ModuleFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn check_execution(flags: &[&str]) {
    let module = ModuleFile::new(&operations());
    for (args, expected) in [
        (&["shl8_1", "i32:128"][..], "0\n"),
        (&["shl8_8", "i32:128"][..], "0\n"),
        (&["shl8_32", "i32:128"][..], "128\n"),
        (&["opcode_group", "i32:184"][..], "1\n"),
        (&["opcode_group", "i32:191"][..], "1\n"),
        (&["opcode_group", "i32:192"][..], "0\n"),
        (&["opcode_register", "i32:184"][..], "0\n"),
        (&["opcode_register", "i32:191"][..], "7\n"),
        (
            &["assemble", "i32:120", "i32:86", "i32:52", "i32:18"][..],
            "305419896\n",
        ),
        (
            &["assemble", "i32:243", "i32:15", "i32:184", "i32:102"][..],
            "1723338739\n",
        ),
        (
            &["assemble", "i32:0", "i32:0", "i32:0", "i32:128"][..],
            "-2147483648\n",
        ),
        (&["shr_wrap_truncate", "i32:-1"][..], "255\n"),
        (&["bits32", "i32:-2023406815"][..], "-2140864479\n"),
        (
            &["bits64", "i64:-9223372036854775808"][..],
            "-9223372036854775807\n",
        ),
        (&["shl32_31", "i32:1"][..], "-2147483648\n"),
        (&["shl32_33", "i32:1"][..], "2\n"),
        (&["shr32_31", "i32:-2147483648"][..], "1\n"),
        (&["shr32_32", "i32:-2147483648"][..], "-2147483648\n"),
        (&["shl64_63", "i64:1"][..], "-9223372036854775808\n"),
        (&["shl64_65", "i64:1"][..], "2\n"),
        (&["shr64_63", "i64:-9223372036854775808"][..], "1\n"),
        (
            &["shr64_64", "i64:-9223372036854775808"][..],
            "-9223372036854775808\n",
        ),
        (&["eq32", "i32:-1", "i32:-1"][..], "1\n"),
        (&["ne32", "i32:-1", "i32:0"][..], "1\n"),
        (&["lt32", "i32:-2147483648", "i32:1"][..], "0\n"),
        (&["lt32", "i32:1", "i32:-2147483648"][..], "1\n"),
        (&["ge32", "i32:-2147483648", "i32:1"][..], "1\n"),
        (
            &[
                "eq64",
                "i64:-9223372036854775808",
                "i64:-9223372036854775808",
            ][..],
            "1\n",
        ),
        (&["eq64", "i64:-9223372036854775808", "i64:0"][..], "0\n"),
        (&["lt64", "i64:-9223372036854775808", "i64:1"][..], "0\n"),
        (&["lt64", "i64:1", "i64:-9223372036854775808"][..], "1\n"),
        (&["ge64", "i64:-9223372036854775808", "i64:1"][..], "1\n"),
        (
            &["truncate64", "i64:-9223372036549334067"][..],
            "305441741\n",
        ),
        (&["truncate8", "i32:305441741"][..], "205\n"),
        (&["truncate1", "i32:2"][..], "0\n"),
        (&["truncate1", "i32:3"][..], "1\n"),
        (&["extend32", "i32:-2147483648"][..], "2147483648\n"),
        (&["extend32", "i32:-1"][..], "4294967295\n"),
        (&["shr8_8", "i32:128"][..], "0\n"),
        (&["shr8_32", "i32:128"][..], "128\n"),
    ] {
        module.check(flags, "execute.mjs", args, expected);
    }
    for (suffix, maximum) in [("1", "i32:1"), ("8", "i32:255"), ("16", "i32:65535")] {
        for (operation, expected) in [
            ("shr", "0\n"),
            ("eqzero", "1\n"),
            ("nezero", "0\n"),
            ("lt", "1\n"),
            ("ge", "0\n"),
            ("extend", "0\n"),
        ] {
            module.check(
                flags,
                "execute.mjs",
                &[&format!("{operation}{suffix}"), maximum],
                expected,
            );
        }
    }
    for (operation, expected) in [
        ("shr8", "64\n"),
        ("eqzero8", "0\n"),
        ("nezero8", "1\n"),
        ("lt8", "0\n"),
        ("ge8", "1\n"),
        ("extend8", "128\n"),
    ] {
        module.check(flags, "execute.mjs", &[operation, "i32:127"], expected);
    }
    for (ne, zero, args, expected) in [
        (false, false, &["run", "i32:1", "i32:255"][..], "1\n"),
        (true, false, &["run", "i32:1", "i32:255"][..], "0\n"),
        (false, false, &["run", "i32:1", "i32:254"][..], "0\n"),
        (true, false, &["run", "i32:1", "i32:254"][..], "1\n"),
        (false, true, &["run", "i32:255"][..], "1\n"),
        (true, true, &["run", "i32:255"][..], "0\n"),
    ] {
        ModuleFile::new(&narrow_comparison(ne, zero)).check(flags, "execute.mjs", args, expected);
    }
    ModuleFile::new(&shared_conversion()).check(flags, "execute.mjs", &["run", "i32:255"], "14\n");
    ModuleFile::new(&conversion_roundtrip()).check(
        flags,
        "execute.mjs",
        &["run", "i32:305441791"],
        "510\n",
    );
    ModuleFile::new(&load_conversion()).check(
        flags,
        "execute-memory.mjs",
        &["state:ffa55a"],
        "262\nstate:00a55a\n",
    );
    let boundary = ModuleFile::new(&shared_boundary());
    boundary.check(
        flags,
        "execute-tail.mjs",
        &[
            "run",
            "ffa55a",
            "receive:i64:-9223372036854775808",
            "i32:255",
        ],
        "receive(0,0,1,0) 00a55a\nreturn -9223372036854775808\nstate 00a55a\n",
    );
    boundary.check(
        flags,
        "execute-tail.mjs",
        &["run", "ffa55a", "receive:i64:17", "i32:127"],
        "receive(128,64,0,128) 80a55a\nreturn 17\nstate 80a55a\n",
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn integer_operations_execute_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn integer_operations_execute_in_v8_optimizing() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
