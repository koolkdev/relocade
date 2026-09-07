use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use wasm86_compiler::{
    FunctionBuilder, IntType, Mem, MemoryImport, Program, Signature, Type, Val, I32, I64, I8,
};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

fn with_memories<T: IntType>(
    names: &[&str],
    parameters: usize,
    build: impl FnOnce(&mut FunctionBuilder<'_>, &[Mem]) -> Val<T>,
) -> Vec<u8> {
    let mut program = Program::new();
    let memories: Vec<_> = names
        .iter()
        .map(|name| {
            program.import_memory(MemoryImport {
                module: "test".into(),
                name: (*name).into(),
                minimum: 1,
                maximum: None,
            })
        })
        .collect();
    let function = program.declare(Signature {
        parameters: vec![Type::I32; parameters],
        result: T::TYPE,
    });
    let mut body = program.define(function).unwrap();
    let result = build(&mut body, &memories);
    body.return_(&result).unwrap();
    program.export("run", function).unwrap();
    program.compile().unwrap()
}

fn module<T: IntType>(
    parameters: usize,
    build: impl FnOnce(&mut FunctionBuilder<'_>, Mem) -> Val<T>,
) -> Vec<u8> {
    with_memories(&["state"], parameters, |body, memories| {
        build(body, memories[0])
    })
}

fn byte_at_offset(wrap_base: bool) -> Vec<u8> {
    module(1, |body, memory| {
        let base = body.parameter::<I32>(0).unwrap();
        let address = if wrap_base { base.add(1) } else { base };
        body.load_at::<I8>(memory, &address, u32::from(!wrap_base))
            .unwrap()
    })
}

fn wide_snapshot(separate_base: bool, store_offset: u32) -> Vec<u8> {
    module(if separate_base { 2 } else { 1 }, |body, memory| {
        let base = body.parameter::<I32>(0).unwrap();
        let other = body.parameter::<I32>(u32::from(separate_base)).unwrap();
        let loaded = body.load_at::<I64>(memory, &base, 0).unwrap();
        body.store_at::<I32>(memory, &other, store_offset, 9)
            .unwrap();
        loaded
    })
}

fn constant_bases() -> Vec<u8> {
    module(0, |body, memory| {
        let loaded = body.load_at::<I64>(memory, 4, 0).unwrap();
        body.store_at::<I32>(memory, 12, 0, 9).unwrap();
        loaded
    })
}

fn nested_loads(reuse_pointer: bool) -> Vec<u8> {
    module(0, |body, memory| {
        let pointer = body.load::<I32>(memory, 0).unwrap();
        let loaded = body.load_at::<I32>(memory, &pointer, 0).unwrap();
        body.store::<I32>(memory, 0, 12).unwrap();
        body.store::<I32>(memory, 8, 9).unwrap();
        if reuse_pointer {
            loaded.add(&pointer)
        } else {
            loaded
        }
    })
}

fn deferred_load_with_captured_pointer() -> Vec<u8> {
    with_memories(&["state", "other"], 0, |body, memories| {
        let pointer = body.load::<I32>(memories[0], 0).unwrap();
        let loaded = body.load_at::<I32>(memories[1], &pointer, 0).unwrap();
        body.store::<I32>(memories[0], 0, 12).unwrap();
        loaded
    })
}

fn store_through_snapshot() -> Vec<u8> {
    module(0, |body, memory| {
        let pointer = body.load::<I32>(memory, 0).unwrap();
        body.store::<I32>(memory, 0, 12).unwrap();
        body.store_at::<I32>(memory, &pointer, 0, 9).unwrap();
        body.value::<I32>(7).unwrap()
    })
}

fn shared_address() -> Vec<u8> {
    module(1, |body, memory| {
        let address = body.parameter::<I32>(0).unwrap().add(4);
        let loaded = body.load_at::<I32>(memory, &address, 0).unwrap();
        body.store_at::<I32>(memory, &address, 4, 9).unwrap();
        body.store_at::<I32>(memory, &address, 8, 10).unwrap();
        loaded
    })
}

fn disjoint_load_trap() -> Vec<u8> {
    module(1, |body, memory| {
        let base = body.parameter::<I32>(0).unwrap();
        let loaded = body.load_at::<I32>(memory, &base, 65536).unwrap();
        body.store_at::<I32>(memory, &base, 0, 9).unwrap();
        loaded
    })
}

fn separate_memories() -> Vec<u8> {
    with_memories(&["state", "other"], 1, |body, memories| {
        let base = body.parameter::<I32>(0).unwrap();
        let loaded = body.load_at::<I32>(memories[0], &base, 0).unwrap();
        body.store_at::<I32>(memories[1], &base, 0, 9).unwrap();
        loaded
    })
}

fn unused_address_chain() -> Vec<u8> {
    with_memories(&["unused", "state", "other"], 1, |body, memories| {
        let base = body.parameter::<I32>(0).unwrap();
        let pointer = body.load_at::<I32>(memories[1], &base, 0).unwrap();
        let _unused = body.load_at::<I8>(memories[2], &pointer, 0).unwrap();
        body.store::<I32>(memories[2], 0, 9).unwrap();
        body.value::<I32>(7).unwrap()
    })
}

#[derive(Debug, PartialEq)]
enum Access {
    Load(u32, u64),
    Store(u32, u64),
}

#[derive(Default)]
struct Code {
    accesses: Vec<Access>,
    imports: Vec<String>,
    additions: usize,
    locals: u32,
    writes: usize,
}

fn inspect(bytes: &[u8]) -> Code {
    Validator::new().validate_all(bytes).unwrap();
    let mut code = Code::default();
    let mut bodies = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.unwrap() {
            Payload::ImportSection(imports) => {
                for import in imports {
                    let import = import.unwrap();
                    if matches!(import.ty, TypeRef::Memory(_)) {
                        code.imports.push(import.name.to_owned());
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                bodies += 1;
                for local in body.get_locals_reader().unwrap() {
                    code.locals += local.unwrap().0;
                }
                let mut operators = body.get_operators_reader().unwrap();
                while !operators.eof() {
                    match operators.read().unwrap() {
                        Operator::I32Load8U { memarg }
                        | Operator::I32Load { memarg }
                        | Operator::I64Load { memarg } => code
                            .accesses
                            .push(Access::Load(memarg.memory, memarg.offset)),
                        Operator::I32Store { memarg } => code
                            .accesses
                            .push(Access::Store(memarg.memory, memarg.offset)),
                        Operator::LocalSet { .. } | Operator::LocalTee { .. } => {
                            code.writes += 1;
                        }
                        Operator::I32Add => code.additions += 1,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    assert_eq!(bodies, 1);
    code
}

#[test]
fn wrapping_address_arithmetic_stays_separate_from_the_memory_offset() {
    let wrapped = inspect(&byte_at_offset(true));
    assert_eq!(wrapped.additions, 1);
    assert_eq!(wrapped.accesses, [Access::Load(0, 0)]);
    let offset = inspect(&byte_at_offset(false));
    assert_eq!(offset.additions, 0);
    assert_eq!(offset.accesses, [Access::Load(0, 1)]);
}

#[test]
fn overlapping_or_unknown_addresses_preserve_the_load_snapshot() {
    for bytes in [wide_snapshot(false, 4), wide_snapshot(true, 8)] {
        let code = inspect(&bytes);
        assert!(matches!(
            code.accesses.as_slice(),
            [Access::Load(..), Access::Store(..)]
        ));
        assert_eq!((code.locals, code.writes), (1, 1));
    }
    for bytes in [
        wide_snapshot(false, 8),
        constant_bases(),
        disjoint_load_trap(),
    ] {
        let code = inspect(&bytes);
        assert!(matches!(
            code.accesses.as_slice(),
            [Access::Store(..), Access::Load(..)]
        ));
        assert_eq!((code.locals, code.writes), (0, 0));
    }
}

#[test]
fn loaded_addresses_are_demanded_where_their_access_is_evaluated() {
    for reuse_pointer in [false, true] {
        let code = inspect(&nested_loads(reuse_pointer));
        assert_eq!(
            code.accesses,
            [
                Access::Load(0, 0),
                Access::Load(0, 0),
                Access::Store(0, 0),
                Access::Store(0, 8)
            ]
        );
        assert_eq!(code.writes, if reuse_pointer { 2 } else { 1 });
    }
    let code = inspect(&deferred_load_with_captured_pointer());
    assert_eq!(
        code.accesses,
        [Access::Load(0, 0), Access::Store(0, 0), Access::Load(1, 0)]
    );
    assert_eq!((code.locals, code.writes), (1, 1));
    let code = inspect(&store_through_snapshot());
    assert_eq!(
        code.accesses,
        [Access::Load(0, 0), Access::Store(0, 0), Access::Store(0, 0)]
    );
    assert_eq!((code.locals, code.writes), (1, 1));
}

#[test]
fn a_computed_address_is_shared_across_load_and_store_roots() {
    let code = inspect(&shared_address());
    assert_eq!(
        code.accesses,
        [Access::Store(0, 4), Access::Store(0, 8), Access::Load(0, 0)]
    );
    assert_eq!((code.additions, code.locals, code.writes), (1, 1, 1));
}

#[test]
fn address_dependencies_preserve_imports_without_forcing_unused_reads() {
    let live = inspect(&separate_memories());
    assert_eq!(live.imports, ["state", "other"]);
    assert_eq!(live.accesses, [Access::Store(1, 0), Access::Load(0, 0)]);
    assert_eq!(live.locals, 0);
    let unused = inspect(&unused_address_chain());
    assert_eq!(unused.imports, ["state", "other"]);
    assert_eq!(unused.accesses, [Access::Store(1, 0)]);
    assert_eq!((unused.locals, unused.writes), (0, 0));
}

struct ModuleFile(PathBuf);

impl ModuleFile {
    fn new(bytes: &[u8]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "wasm86-address-{}-{}.wasm",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, bytes).unwrap();
        Self(path)
    }
}

impl Drop for ModuleFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn check(flags: &[&str], bytes: Vec<u8>, memories: &[&str], args: &[&str], expected: &str) {
    let module = ModuleFile::new(&bytes);
    let output = Command::new("node")
        .args(flags)
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/execute-memory.mjs"
        ))
        .arg(&module.0)
        .args(memories)
        .arg("--")
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
        "arguments {args:?}, V8 flags {flags:?}"
    );
}

fn check_execution(flags: &[&str]) {
    check(
        flags,
        byte_at_offset(false),
        &["state:a5b85ac3"],
        &["i32:0"],
        "184\nstate:a5b85ac3\n",
    );
    check(
        flags,
        byte_at_offset(true),
        &["state:a5b85ac3"],
        &["i32:-1"],
        "165\nstate:a5b85ac3\n",
    );
    check(
        flags,
        byte_at_offset(false),
        &["state:a5b85ac3"],
        &["i32:-1"],
        "trap\nstate:a5b85ac3\n",
    );
    let initial = "state:a55ac33c070000000000008003000000";
    check(
        flags,
        wide_snapshot(false, 4),
        &[initial],
        &["i32:4"],
        "-9223372036854775801\nstate:a55ac33c070000000900000003000000\n",
    );
    check(
        flags,
        wide_snapshot(false, 8),
        &[initial],
        &["i32:4"],
        "-9223372036854775801\nstate:a55ac33c070000000000008009000000\n",
    );
    check(
        flags,
        wide_snapshot(true, 0),
        &[initial],
        &["i32:4", "i32:4"],
        "-9223372036854775801\nstate:a55ac33c090000000000008003000000\n",
    );
    check(
        flags,
        wide_snapshot(true, 8),
        &[initial],
        &["i32:4", "i32:0"],
        "-9223372036854775801\nstate:a55ac33c070000000900000003000000\n",
    );
    check(
        flags,
        constant_bases(),
        &[initial],
        &[],
        "-9223372036854775801\nstate:a55ac33c070000000000008009000000\n",
    );
    for (reuse_pointer, expected) in [
        (false, "7\nstate:0c000000a55ac33c0900000005000000\n"),
        (true, "15\nstate:0c000000a55ac33c0900000005000000\n"),
    ] {
        check(
            flags,
            nested_loads(reuse_pointer),
            &["state:08000000a55ac33c0700000005000000"],
            &[],
            expected,
        );
    }
    check(
        flags,
        deferred_load_with_captured_pointer(),
        &[
            "state:08000000a55a",
            "other:a55ac33ca55ac33c0700000005000000",
        ],
        &[],
        "7\nstate:0c000000a55a\nother:a55ac33ca55ac33c0700000005000000\n",
    );
    check(
        flags,
        store_through_snapshot(),
        &["state:08000000a55ac33c0700000005000000"],
        &[],
        "7\nstate:0c000000a55ac33c0900000005000000\n",
    );
    check(
        flags,
        shared_address(),
        &["state:a55ac33c070000000500000003000000"],
        &["i32:0"],
        "7\nstate:a55ac33c07000000090000000a000000\n",
    );
    // The disjoint store executes before the deferred load traps.
    check(
        flags,
        disjoint_load_trap(),
        &["state:07000000a55ac33c"],
        &["i32:0"],
        "trap\nstate:09000000a55ac33c\n",
    );
    check(
        flags,
        separate_memories(),
        &["state:07000000a55a", "other:050000005aa5"],
        &["i32:0"],
        "7\nstate:07000000a55a\nother:090000005aa5\n",
    );
    check(
        flags,
        unused_address_chain(),
        &["state:07000000a55a", "other:050000005aa5"],
        &["i32:65536"],
        "7\nstate:07000000a55a\nother:090000005aa5\n",
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn computed_addresses_execute_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn computed_addresses_execute_in_v8_optimizing() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
