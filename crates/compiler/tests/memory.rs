use std::{
    fmt::Write,
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use wasm86_compiler::{
    FunctionBuilder, IntType, Mem, MemoryImport, MemoryInt, Program, Signature, Type, Val, I16,
    I32, I64, I8,
};
use wasmparser::{Operator, Parser, Payload, TypeRef};

fn import(program: &mut Program, name: &str) -> Mem {
    program.import_memory(MemoryImport {
        module: "test".into(),
        name: name.into(),
        minimum: 1,
        maximum: None,
    })
}

fn with_memories<T: IntType>(
    names: &[&str],
    build: impl FnOnce(&mut FunctionBuilder<'_>, &[Mem]) -> Val<T>,
) -> Vec<u8> {
    let mut program = Program::new();
    let memories: Vec<_> = names
        .iter()
        .map(|name| import(&mut program, name))
        .collect();
    let function = program.declare(Signature {
        parameters: vec![],
        result: T::TYPE,
    });
    let mut body = program.define(function).unwrap();
    let result = build(&mut body, &memories);
    body.return_(&result).unwrap();
    program.export("run", function).unwrap();
    program.compile().unwrap()
}

fn module<T: IntType>(build: impl FnOnce(&mut FunctionBuilder<'_>, Mem) -> Val<T>) -> Vec<u8> {
    with_memories(&["state"], |body, memories| build(body, memories[0]))
}

fn snapshot_and_fresh_read() -> Vec<u8> {
    module(|body, memory| {
        let before = body.load::<I32>(memory, 0).unwrap();
        body.store::<I32>(memory, 0, 9).unwrap();
        let after = body.load::<I32>(memory, 0).unwrap();
        before.add(&after)
    })
}

fn shared_snapshot(return_load: bool) -> Vec<u8> {
    module(|body, memory| {
        let loaded = body.load::<I32>(memory, 0).unwrap();
        let shared = loaded.add(1);
        body.store::<I32>(memory, 8, 1).unwrap();
        body.store(memory, 4, &shared).unwrap();
        body.store(memory, 12, &shared).unwrap();
        body.store::<I32>(memory, 0, 9).unwrap();
        if return_load {
            loaded.add(&shared)
        } else {
            shared
        }
    })
}

fn read_modify_write() -> Vec<u8> {
    module(|body, memory| {
        let loaded = body.load::<I32>(memory, 0).unwrap();
        body.store(memory, 0, loaded.add(1)).unwrap();
        body.value::<I32>(7).unwrap()
    })
}

fn wide_snapshot(store_offset: u32) -> Vec<u8> {
    module(|body, memory| {
        let loaded = body.load::<I64>(memory, 0).unwrap();
        body.store::<I32>(memory, store_offset, 9).unwrap();
        loaded
    })
}

fn wide_write_and_fresh_read() -> Vec<u8> {
    module(|body, memory| {
        let before = body.load::<I64>(memory, 0).unwrap();
        body.store::<I64>(memory, 0, u64::MAX).unwrap();
        let after = body.load::<I64>(memory, 0).unwrap();
        before.add(&after)
    })
}

fn high_offset_load(has_overlapping_store: bool) -> Vec<u8> {
    module(|body, memory| {
        let loaded = body.load::<I64>(memory, u32::MAX).unwrap();
        body.store::<I32>(memory, 0, 9).unwrap();
        if has_overlapping_store {
            // The range ends exceed u32::MAX, but these accesses still overlap.
            body.store::<I8>(memory, u32::MAX, 1).unwrap();
        }
        loaded
    })
}

fn increment_narrow<T: MemoryInt>(offset: u32) -> Vec<u8> {
    module(|body, memory| {
        let loaded = body.load::<T>(memory, offset).unwrap();
        body.store(memory, offset, loaded.add(1)).unwrap();
        loaded
    })
}

fn unused_load() -> Vec<u8> {
    with_memories(&["unused", "state"], |body, memories| {
        let _unused = body.load::<I32>(memories[1], 65536).unwrap();
        body.value::<I32>(7).unwrap()
    })
}

fn trapping_load(has_overlapping_store: bool) -> Vec<u8> {
    module(|body, memory| {
        let loaded = body.load::<I32>(memory, 65536).unwrap();
        body.store::<I32>(memory, 0, 9).unwrap();
        if has_overlapping_store {
            body.store::<I32>(memory, 65536, 1).unwrap();
        }
        loaded
    })
}

fn trapping_store() -> Vec<u8> {
    module(|body, memory| {
        body.store::<I32>(memory, 0, 1).unwrap();
        body.store::<I32>(memory, 65536, 2).unwrap();
        body.store::<I32>(memory, 0, 3).unwrap();
        body.value::<I32>(7).unwrap()
    })
}

fn different_memories() -> Vec<u8> {
    with_memories(&["unused", "other", "state"], |body, memories| {
        let loaded = body.load::<I32>(memories[2], 0).unwrap();
        body.store::<I32>(memories[1], 0, 9).unwrap();
        loaded
    })
}

#[derive(Debug, Eq, PartialEq)]
enum Access {
    Load(u8, u64),
    Store(u8, u64),
}

#[derive(Default)]
struct Code {
    memories: Vec<String>,
    accesses: Vec<Access>,
    locals: u32,
    local_writes: usize,
    additions: usize,
    masks: usize,
}

fn inspect(bytes: &[u8]) -> Code {
    let mut code = Code::default();
    let mut bodies = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.unwrap() {
            Payload::ImportSection(imports) => {
                for import in imports {
                    let import = import.unwrap();
                    if matches!(import.ty, TypeRef::Memory(_)) {
                        code.memories.push(import.name.to_owned());
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
                    let access = match operators.read().unwrap() {
                        Operator::I32Load8U { memarg } => Access::Load(1, memarg.offset),
                        Operator::I32Load16U { memarg } => Access::Load(2, memarg.offset),
                        Operator::I32Load { memarg } => Access::Load(4, memarg.offset),
                        Operator::I64Load { memarg } => Access::Load(8, memarg.offset),
                        Operator::I32Store8 { memarg } => Access::Store(1, memarg.offset),
                        Operator::I32Store16 { memarg } => Access::Store(2, memarg.offset),
                        Operator::I32Store { memarg } => Access::Store(4, memarg.offset),
                        Operator::I64Store { memarg } => Access::Store(8, memarg.offset),
                        Operator::LocalSet { .. } | Operator::LocalTee { .. } => {
                            code.local_writes += 1;
                            continue;
                        }
                        Operator::I32And => {
                            code.masks += 1;
                            continue;
                        }
                        Operator::I32Add | Operator::I64Add => {
                            code.additions += 1;
                            continue;
                        }
                        _ => continue,
                    };
                    code.accesses.push(access);
                }
            }
            _ => {}
        }
    }
    assert_eq!(bodies, 1);
    code
}

#[test]
fn fresh_reads_observe_writes_without_changing_an_earlier_snapshot() {
    for (bytes, width) in [
        (snapshot_and_fresh_read(), 4),
        (wide_write_and_fresh_read(), 8),
    ] {
        let code = inspect(&bytes);
        assert_eq!(
            code.accesses,
            [
                Access::Load(width, 0),
                Access::Store(width, 0),
                Access::Load(width, 0)
            ]
        );
        assert_eq!(code.local_writes, 1);
    }
}

#[test]
fn shared_values_are_evaluated_at_their_first_use() {
    for (return_load, locals, additions) in [(false, 1, 1), (true, 2, 2)] {
        let code = inspect(&shared_snapshot(return_load));
        assert_eq!(
            code.accesses,
            [
                Access::Store(4, 8),
                Access::Load(4, 0),
                Access::Store(4, 4),
                Access::Store(4, 12),
                Access::Store(4, 0),
            ]
        );
        assert_eq!(code.locals, locals);
        assert_eq!(code.local_writes, locals as usize);
        assert_eq!(code.additions, additions);
    }
}

#[test]
fn a_read_modify_write_needs_no_snapshot_local() {
    let code = inspect(&read_modify_write());
    assert_eq!(code.accesses, [Access::Load(4, 0), Access::Store(4, 0)]);
    assert_eq!(code.locals, 0);
    assert_eq!(code.local_writes, 0);
}

#[test]
fn partial_overlap_captures_a_load_and_adjacent_storage_does_not() {
    let partial = inspect(&wide_snapshot(4));
    assert_eq!(partial.accesses, [Access::Load(8, 0), Access::Store(4, 4)]);
    assert_eq!(partial.local_writes, 1);
    let adjacent = inspect(&wide_snapshot(8));
    assert_eq!(adjacent.accesses, [Access::Store(4, 8), Access::Load(8, 0)]);
    assert_eq!(adjacent.local_writes, 0);
}

#[test]
fn narrow_stores_write_only_their_bytes_without_an_extra_mask() {
    for (bytes, width) in [
        (increment_narrow::<I8>(1), 1),
        (increment_narrow::<I16>(1), 2),
    ] {
        let code = inspect(&bytes);
        assert_eq!(
            code.accesses,
            [Access::Load(width, 1), Access::Store(width, 1)]
        );
        assert_eq!(code.masks, 0);
    }
}

#[test]
fn memory_ranges_do_not_wrap_at_the_largest_offset() {
    let disjoint = inspect(&high_offset_load(false));
    assert_eq!(
        disjoint.accesses,
        [Access::Store(4, 0), Access::Load(8, 4294967295)]
    );
    assert_eq!(disjoint.local_writes, 0);
    let overlapping = inspect(&high_offset_load(true));
    assert_eq!(
        overlapping.accesses,
        [
            Access::Load(8, 4294967295),
            Access::Store(4, 0),
            Access::Store(1, 4294967295),
        ]
    );
    assert_eq!(overlapping.local_writes, 1);
}

#[test]
fn unused_loads_keep_their_import_without_emitting_a_read() {
    let code = inspect(&unused_load());
    assert_eq!(code.memories, ["state"]);
    assert!(code.accesses.is_empty());
}

#[test]
fn abandoned_bodies_do_not_retain_memory_imports() {
    let mut program = Program::new();
    let memory = import(&mut program, "abandoned");
    let function = program.declare(Signature {
        parameters: vec![],
        result: Type::I32,
    });
    let mut body = program.define(function).unwrap();
    body.load::<I32>(memory, 0).unwrap();
    drop(body);
    let body = program.define(function).unwrap();
    body.return_(7).unwrap();
    program.export("run", function).unwrap();
    assert!(inspect(&program.compile().unwrap()).memories.is_empty());
}

#[test]
fn different_memories_keep_import_order_and_do_not_alias() {
    let code = inspect(&different_memories());
    assert_eq!(code.memories, ["other", "state"]);
    assert_eq!(code.accesses, [Access::Store(4, 0), Access::Load(4, 0)]);
    assert_eq!(code.local_writes, 0);
}

struct ModuleFile(PathBuf);

impl ModuleFile {
    fn new(bytes: &[u8]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "wasm86-memory-{}-{}.wasm",
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

fn hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").unwrap();
    }
    encoded
}

fn check(
    flags: &[&str],
    name: &str,
    bytes: Vec<u8>,
    result: &str,
    memories: &[(&str, &[u8], &[u8])],
) {
    let module = ModuleFile::new(&bytes);
    let mut expected = format!("{result}\n");
    let mut arguments = Vec::new();
    for (name, initial, final_bytes) in memories {
        assert_eq!(initial.len(), final_bytes.len());
        arguments.push(format!("{name}:{}", hex(initial)));
        expected.push_str(&format!("{name}:{}\n", hex(final_bytes)));
    }
    let output = Command::new("node")
        .args(flags)
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/execute-memory.mjs"
        ))
        .arg(&module.0)
        .args(arguments)
        .output()
        .expect("the explicit V8 lane requires Node.js on PATH");
    assert!(
        output.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        expected,
        "{name}, V8 flags {flags:?}"
    );
}

fn check_state(
    flags: &[&str],
    name: &str,
    bytes: Vec<u8>,
    initial: &[u8],
    result: &str,
    final_bytes: &[u8],
) {
    check(
        flags,
        name,
        bytes,
        result,
        &[("state", initial, final_bytes)],
    );
}

fn check_execution(flags: &[&str]) {
    let initial = &[7, 0, 0, 0, 5, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c];
    let replaced = &[9, 0, 0, 0, 5, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c];
    check_state(
        flags,
        "fresh read",
        snapshot_and_fresh_read(),
        initial,
        "16",
        replaced,
    );
    for (return_load, result) in [(false, "8"), (true, "15")] {
        check_state(
            flags,
            "shared snapshot",
            shared_snapshot(return_load),
            initial,
            result,
            &[9, 0, 0, 0, 8, 0, 0, 0, 1, 0, 0, 0, 8, 0, 0, 0],
        );
    }
    check_state(
        flags,
        "read modify write",
        read_modify_write(),
        initial,
        "7",
        &[8, 0, 0, 0, 5, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c],
    );
    let wide = &[7, 0, 0, 0, 0, 0, 0, 128, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c];
    check_state(
        flags,
        "partial overlap",
        wide_snapshot(4),
        wide,
        "-9223372036854775801",
        &[7, 0, 0, 0, 9, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c],
    );
    check_state(
        flags,
        "adjacent storage",
        wide_snapshot(8),
        wide,
        "-9223372036854775801",
        &[7, 0, 0, 0, 0, 0, 0, 128, 9, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c],
    );
    check_state(
        flags,
        "i64 store and fresh read",
        wide_write_and_fresh_read(),
        wide,
        "-9223372036854775802",
        &[
            255, 255, 255, 255, 255, 255, 255, 255, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c,
        ],
    );
    check_state(
        flags,
        "byte footprint and unsigned wrap",
        increment_narrow::<I8>(1),
        &[0xa5, 255, 0x5a, 0xc3],
        "255",
        &[0xa5, 0, 0x5a, 0xc3],
    );
    check_state(
        flags,
        "halfword footprint and unsigned wrap",
        increment_narrow::<I16>(1),
        &[0xa5, 255, 255, 0x5a, 0xc3],
        "65535",
        &[0xa5, 0, 0, 0x5a, 0xc3],
    );
    check_state(flags, "unused load", unused_load(), initial, "7", initial);
    check_state(
        flags,
        "disjoint load trap",
        trapping_load(false),
        initial,
        "trap",
        replaced,
    );
    check_state(
        flags,
        "captured load trap",
        trapping_load(true),
        initial,
        "trap",
        initial,
    );
    check_state(
        flags,
        "high offset load trap",
        high_offset_load(false),
        initial,
        "trap",
        replaced,
    );
    check_state(
        flags,
        "overlapping high offset trap",
        high_offset_load(true),
        initial,
        "trap",
        initial,
    );
    check_state(
        flags,
        "store order around a trap",
        trapping_store(),
        initial,
        "trap",
        &[1, 0, 0, 0, 5, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c],
    );
    check(
        flags,
        "different memories",
        different_memories(),
        "7",
        &[
            ("state", &[7, 0, 0, 0], &[7, 0, 0, 0]),
            ("other", &[5, 0, 0, 0], &[9, 0, 0, 0]),
        ],
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn memory_effects_execute_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn memory_effects_execute_in_v8_optimizing() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
