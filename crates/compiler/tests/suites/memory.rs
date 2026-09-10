use crate::fixture::Fixture;
use crate::wasm::TestModule;

use wasm86_compiler::{MemoryImport, MemoryInt, Program, Signature, Type, I16, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, TypeRef};

fn snapshot_and_fresh_read() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", INITIAL);
    fixture.function(&[], &[Type::I32], |mut body| {
        let before = body.load::<I32>(state, 0)?;
        body.store::<I32>(state, 0, 9)?;
        let after = body.load::<I32>(state, 0)?;
        body.return_(before.add(after))
    })
}

fn shared_snapshot(return_load: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", INITIAL);
    fixture.function(&[], &[Type::I32], |mut body| {
        let loaded = body.load::<I32>(memory, 0)?;
        let shared = loaded.add(1);
        body.store::<I32>(memory, 8, 1)?;
        body.store(memory, 4, &shared)?;
        body.store(memory, 12, &shared)?;
        body.store::<I32>(memory, 0, 9)?;
        let result = if return_load {
            loaded.add(&shared)
        } else {
            shared
        };
        body.return_(result)
    })
}

fn read_modify_write() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", INITIAL);
    fixture.function(&[], &[Type::I32], |mut body| {
        let loaded = body.load::<I32>(memory, 0)?;
        body.store(memory, 0, loaded.add(1))?;
        let result = body.value::<I32>(7)?;
        body.return_(result)
    })
}

fn wide_snapshot(store_offset: u32) -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", WIDE);
    fixture.function(&[], &[Type::I64], |mut body| {
        let loaded = body.load::<I64>(memory, 0)?;
        body.store::<I32>(memory, store_offset, 9)?;
        let result = loaded;
        body.return_(result)
    })
}

fn wide_write_and_fresh_read() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", WIDE);
    fixture.function(&[], &[Type::I64], |mut body| {
        let before = body.load::<I64>(memory, 0)?;
        body.store::<I64>(memory, 0, u64::MAX)?;
        let after = body.load::<I64>(memory, 0)?;
        let result = before.add(&after);
        body.return_(result)
    })
}

fn high_offset_load(has_overlapping_store: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", INITIAL);
    fixture.function(&[], &[Type::I64], |mut body| {
        let loaded = body.load::<I64>(memory, u32::MAX)?;
        body.store::<I32>(memory, 0, 9)?;
        if has_overlapping_store {
            // The range ends exceed u32::MAX, but these accesses still overlap.
            body.store::<I8>(memory, u32::MAX, 1)?;
        }
        body.return_(loaded)
    })
}

fn increment_narrow<T: MemoryInt>(initial: &[u8], offset: u32) -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", initial);
    fixture.function(&[], &[T::TYPE], |mut body| {
        let loaded = body.load::<T>(memory, offset)?;
        body.store(memory, offset, loaded.add(1))?;
        let result = loaded;
        body.return_(result)
    })
}

fn unused_load() -> TestModule {
    let mut fixture = Fixture::new();
    fixture.memory("unused", &[]);
    let memory = fixture.memory("state", INITIAL);
    fixture.function(&[], &[Type::I32], |mut body| {
        let _unused = body.load::<I32>(memory, 65536)?;
        let result = body.value::<I32>(7)?;
        body.return_(result)
    })
}

fn trapping_load(has_overlapping_store: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", INITIAL);
    fixture.function(&[], &[Type::I32], |mut body| {
        let loaded = body.load::<I32>(memory, 65536)?;
        body.store::<I32>(memory, 0, 9)?;
        if has_overlapping_store {
            body.store::<I32>(memory, 65536, 1)?;
        }
        body.return_(loaded)
    })
}

fn different_memories() -> TestModule {
    let mut fixture = Fixture::new();
    fixture.memory("unused", &[]);
    let other = fixture.memory("other", &[5, 0, 0, 0]);
    let memory = fixture.memory("state", &[7, 0, 0, 0]);
    fixture.function(&[], &[Type::I32], |mut body| {
        let loaded = body.load::<I32>(memory, 0)?;
        body.store::<I32>(other, 0, 9)?;
        let result = loaded;
        body.return_(result)
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
        let code = inspect(bytes.bytes());
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
        let code = inspect(shared_snapshot(return_load).bytes());
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
    let code = inspect(read_modify_write().bytes());
    assert_eq!(code.accesses, [Access::Load(4, 0), Access::Store(4, 0)]);
    assert_eq!(code.locals, 0);
    assert_eq!(code.local_writes, 0);
}

#[test]
fn partial_overlap_captures_a_load_and_adjacent_storage_does_not() {
    let partial = inspect(wide_snapshot(4).bytes());
    assert_eq!(partial.accesses, [Access::Load(8, 0), Access::Store(4, 4)]);
    assert_eq!(partial.local_writes, 1);
    let adjacent = inspect(wide_snapshot(8).bytes());
    assert_eq!(adjacent.accesses, [Access::Store(4, 8), Access::Load(8, 0)]);
    assert_eq!(adjacent.local_writes, 0);
}

#[test]
fn narrow_stores_write_only_their_bytes_without_an_extra_mask() {
    for (bytes, width) in [
        (increment_narrow::<I8>(&[0xa5, 255, 0x5a, 0xc3], 1), 1),
        (increment_narrow::<I16>(&[0xa5, 255, 255, 0x5a, 0xc3], 1), 2),
    ] {
        let code = inspect(bytes.bytes());
        assert_eq!(
            code.accesses,
            [Access::Load(width, 1), Access::Store(width, 1)]
        );
        assert_eq!(code.masks, 0);
    }
}

#[test]
fn memory_ranges_do_not_wrap_at_the_largest_offset() {
    let disjoint = inspect(high_offset_load(false).bytes());
    assert_eq!(
        disjoint.accesses,
        [Access::Store(4, 0), Access::Load(8, 4294967295)]
    );
    assert_eq!(disjoint.local_writes, 0);
    let overlapping = inspect(high_offset_load(true).bytes());
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
    let code = inspect(unused_load().bytes());
    assert_eq!(code.memories, ["state"]);
    assert!(code.accesses.is_empty());
}

#[test]
fn abandoned_bodies_do_not_retain_memory_imports() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "abandoned".into(),
        minimum: 1,
        maximum: None,
    });
    let function = program.declare(Signature {
        parameters: vec![],
        results: vec![Type::I32],
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
    let code = inspect(different_memories().bytes());
    assert_eq!(code.memories, ["other", "state"]);
    assert_eq!(code.accesses, [Access::Store(4, 0), Access::Load(4, 0)]);
    assert_eq!(code.local_writes, 0);
}

const INITIAL: &[u8] = &[7, 0, 0, 0, 5, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c];
const REPLACED: &[u8] = &[9, 0, 0, 0, 5, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c];
const WIDE: &[u8] = &[7, 0, 0, 0, 0, 0, 0, 128, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c];

#[test]
fn fresh_reads_observe_stores_while_prior_reads_keep_their_snapshot() {
    let module = snapshot_and_fresh_read();
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 16);
    assert_eq!(&instance.memory("state")[..REPLACED.len()], REPLACED);
}

#[test]
fn shared_snapshots_keep_their_value_across_writes() {
    for (return_load, expected) in [(false, 8), (true, 15)] {
        let module = shared_snapshot(return_load);
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(()).unwrap(), expected);
        assert_eq!(
            &instance.memory("state")[..16],
            &[9, 0, 0, 0, 8, 0, 0, 0, 1, 0, 0, 0, 8, 0, 0, 0]
        );
        assert!(instance.callbacks().is_empty());
    }
}

#[test]
fn read_modify_write_returns_the_prior_value() {
    let module = read_modify_write();
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 7);
    assert_eq!(
        &instance.memory("state")[..16],
        &[8, 0, 0, 0, 5, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c]
    );
    assert!(instance.callbacks().is_empty());
}

#[test]
fn wide_snapshots_survive_overlapping_and_adjacent_stores() {
    for (offset, expected) in [
        (
            4,
            &[7, 0, 0, 0, 9, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c],
        ),
        (
            8,
            &[7, 0, 0, 0, 0, 0, 0, 128, 9, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c],
        ),
    ] {
        let module = wide_snapshot(offset);
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i64>(()).unwrap(), -9223372036854775801);
        assert_eq!(&instance.memory("state")[..expected.len()], expected);
        assert!(instance.callbacks().is_empty());
    }
}

#[test]
fn wide_stores_are_visible_to_fresh_reads() {
    let module = wide_write_and_fresh_read();
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i64>(()).unwrap(), -9223372036854775802);
    assert_eq!(
        &instance.memory("state")[..16],
        &[255, 255, 255, 255, 255, 255, 255, 255, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c]
    );
    assert!(instance.callbacks().is_empty());
}

#[test]
fn byte_increment_wraps_without_touching_adjacent_bytes() {
    let module = increment_narrow::<I8>(&[0xa5, 255, 0x5a, 0xc3], 1);
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 255);
    assert_eq!(&instance.memory("state")[..4], &[0xa5, 0, 0x5a, 0xc3]);
    assert!(instance.callbacks().is_empty());
}

#[test]
fn halfword_increment_wraps_without_touching_adjacent_bytes() {
    let module = increment_narrow::<I16>(&[0xa5, 255, 255, 0x5a, 0xc3], 1);
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 65535);
    assert_eq!(&instance.memory("state")[..5], &[0xa5, 0, 0, 0x5a, 0xc3]);
    assert!(instance.callbacks().is_empty());
}

#[test]
fn unused_loads_do_not_trap() {
    let module = unused_load();
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 7);
    assert_eq!(&instance.memory("state")[..INITIAL.len()], INITIAL);
    assert!(instance.callbacks().is_empty());
}

#[test]
fn load_traps_follow_snapshot_and_aliasing_order() {
    for (module, expected) in [
        (trapping_load(false), REPLACED),
        (trapping_load(true), INITIAL),
        (high_offset_load(false), REPLACED),
        (high_offset_load(true), INITIAL),
    ] {
        let mut instance = module.instantiate();
        assert!(instance.call_values("run", &[]).is_err());
        assert_eq!(&instance.memory("state")[..expected.len()], expected);
        assert!(instance.callbacks().is_empty());
    }
}

#[test]
fn store_traps_preserve_only_preceding_stores() {
    let module = {
        let mut fixture = Fixture::new();
        let memory = fixture.memory("state", INITIAL);
        fixture.function(&[], &[Type::I32], |mut body| {
            body.store::<I32>(memory, 0, 1)?;
            body.store::<I32>(memory, 65536, 2)?;
            body.store::<I32>(memory, 0, 3)?;
            let result = body.value::<I32>(7)?;
            body.return_(result)
        })
    };
    let mut instance = module.instantiate();
    assert!(instance.call::<i32>(()).is_err());
    assert_eq!(
        &instance.memory("state")[..16],
        &[1, 0, 0, 0, 5, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c]
    );
    assert!(instance.callbacks().is_empty());
}

#[test]
fn stores_in_one_memory_do_not_change_another_memory() {
    let module = different_memories();
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 7);
    assert_eq!(&instance.memory("state")[..4], &[7, 0, 0, 0]);
    assert_eq!(&instance.memory("other")[..4], &[9, 0, 0, 0]);
    assert!(instance.callbacks().is_empty());
}
