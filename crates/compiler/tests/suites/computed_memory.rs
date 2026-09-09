use crate::fixture::Fixture;
use crate::wasm::TestModule;

use wasm86_compiler::{Type, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

const WIDE: &[u8] = &[
    0xa5, 0x5a, 0xc3, 0x3c, 7, 0, 0, 0, 0, 0, 0, 0x80, 3, 0, 0, 0,
];
const POINTER: &[u8] = &[8, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c, 7, 0, 0, 0, 5, 0, 0, 0];

fn byte_at_offset(wrap_base: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0xa5, 0xb8, 0x5a, 0xc3]);
    fixture.function(&[Type::I32], Some(Type::I8), |mut body| {
        let base = body.parameter::<I32>(0)?;
        let address = if wrap_base { base.add(1) } else { base };
        let byte = body.load_at::<I8>(memory, &address, u32::from(!wrap_base))?;
        body.return_(byte)
    })
}

fn wide_snapshot(separate_base: bool, store_offset: u32) -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", WIDE);
    fixture.function(
        &vec![Type::I32; if separate_base { 2 } else { 1 }],
        Some(Type::I64),
        |mut body| {
            let base = body.parameter::<I32>(0)?;
            let other = body.parameter::<I32>(u32::from(separate_base))?;
            let loaded = body.load_at::<I64>(memory, &base, 0)?;
            body.store_at::<I32>(memory, &other, store_offset, 9)?;
            body.return_(loaded)
        },
    )
}

fn constant_bases() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", WIDE);
    fixture.function(&[], Some(Type::I64), |mut body| {
        let loaded = body.load_at::<I64>(memory, 4, 0)?;
        body.store_at::<I32>(memory, 12, 0, 9)?;
        body.return_(loaded)
    })
}

fn nested_loads(reuse_pointer: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", POINTER);
    fixture.function(&[], Some(Type::I32), |mut body| {
        let pointer = body.load::<I32>(memory, 0)?;
        let loaded = body.load_at::<I32>(memory, &pointer, 0)?;
        body.store::<I32>(memory, 0, 12)?;
        body.store::<I32>(memory, 8, 9)?;
        body.return_(if reuse_pointer {
            loaded.add(&pointer)
        } else {
            loaded
        })
    })
}

fn deferred_load_with_captured_pointer() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[8, 0, 0, 0, 0xa5, 0x5a]);
    let other = fixture.memory(
        "other",
        &[
            0xa5, 0x5a, 0xc3, 0x3c, 0xa5, 0x5a, 0xc3, 0x3c, 7, 0, 0, 0, 5, 0, 0, 0,
        ],
    );
    fixture.function(&[], Some(Type::I32), |mut body| {
        let pointer = body.load::<I32>(state, 0)?;
        let loaded = body.load_at::<I32>(other, &pointer, 0)?;
        body.store::<I32>(state, 0, 12)?;
        body.return_(loaded)
    })
}

fn store_through_snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", POINTER);
    fixture.function(&[], Some(Type::I32), |mut body| {
        let pointer = body.load::<I32>(memory, 0)?;
        body.store::<I32>(memory, 0, 12)?;
        body.store_at::<I32>(memory, &pointer, 0, 9)?;
        body.return_(7)
    })
}

fn shared_address() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory(
        "state",
        &[0xa5, 0x5a, 0xc3, 0x3c, 7, 0, 0, 0, 5, 0, 0, 0, 3, 0, 0, 0],
    );
    fixture.function(&[Type::I32], Some(Type::I32), |mut body| {
        let address = body.parameter::<I32>(0)?.add(4);
        let loaded = body.load_at::<I32>(memory, &address, 0)?;
        body.store_at::<I32>(memory, &address, 4, 9)?;
        body.store_at::<I32>(memory, &address, 8, 10)?;
        body.return_(loaded)
    })
}

fn disjoint_load_trap() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[7, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c]);
    fixture.function(&[Type::I32], Some(Type::I32), |mut body| {
        let base = body.parameter::<I32>(0)?;
        let loaded = body.load_at::<I32>(memory, &base, 65536)?;
        body.store_at::<I32>(memory, &base, 0, 9)?;
        body.return_(loaded)
    })
}

fn separate_memories() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0xa5, 0x5a]);
    let other = fixture.memory("other", &[5, 0, 0, 0, 0x5a, 0xa5]);
    fixture.function(&[Type::I32], Some(Type::I32), |mut body| {
        let base = body.parameter::<I32>(0)?;
        let loaded = body.load_at::<I32>(state, &base, 0)?;
        body.store_at::<I32>(other, &base, 0, 9)?;
        body.return_(loaded)
    })
}

fn unused_address_chain() -> TestModule {
    let mut fixture = Fixture::new();
    fixture.memory("unused", &[]);
    let state = fixture.memory("state", &[7, 0, 0, 0, 0xa5, 0x5a]);
    let other = fixture.memory("other", &[5, 0, 0, 0, 0x5a, 0xa5]);
    fixture.function(&[Type::I32], Some(Type::I32), |mut body| {
        let base = body.parameter::<I32>(0)?;
        let pointer = body.load_at::<I32>(state, &base, 0)?;
        let _unused = body.load_at::<I8>(other, &pointer, 0)?;
        body.store::<I32>(other, 0, 9)?;
        body.return_(7)
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
    let wrapped = inspect(byte_at_offset(true).bytes());
    assert_eq!(wrapped.additions, 1);
    assert_eq!(wrapped.accesses, [Access::Load(0, 0)]);
    let offset = inspect(byte_at_offset(false).bytes());
    assert_eq!(offset.additions, 0);
    assert_eq!(offset.accesses, [Access::Load(0, 1)]);
}

#[test]
fn overlapping_or_unknown_addresses_preserve_the_load_snapshot() {
    for bytes in [wide_snapshot(false, 4), wide_snapshot(true, 8)] {
        let code = inspect(bytes.bytes());
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
        let code = inspect(bytes.bytes());
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
        let code = inspect(nested_loads(reuse_pointer).bytes());
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
    let code = inspect(deferred_load_with_captured_pointer().bytes());
    assert_eq!(
        code.accesses,
        [Access::Load(0, 0), Access::Store(0, 0), Access::Load(1, 0)]
    );
    assert_eq!((code.locals, code.writes), (1, 1));
    let code = inspect(store_through_snapshot().bytes());
    assert_eq!(
        code.accesses,
        [Access::Load(0, 0), Access::Store(0, 0), Access::Store(0, 0)]
    );
    assert_eq!((code.locals, code.writes), (1, 1));
}

#[test]
fn a_computed_address_is_shared_across_load_and_store_roots() {
    let code = inspect(shared_address().bytes());
    assert_eq!(
        code.accesses,
        [Access::Store(0, 4), Access::Store(0, 8), Access::Load(0, 0)]
    );
    assert_eq!((code.additions, code.locals, code.writes), (1, 1, 1));
}

#[test]
fn address_dependencies_preserve_imports_without_forcing_unused_reads() {
    let live = inspect(separate_memories().bytes());
    assert_eq!(live.imports, ["state", "other"]);
    assert_eq!(live.accesses, [Access::Store(1, 0), Access::Load(0, 0)]);
    assert_eq!(live.locals, 0);
    let unused = inspect(unused_address_chain().bytes());
    assert_eq!(unused.imports, ["state", "other"]);
    assert_eq!(unused.accesses, [Access::Store(1, 0)]);
    assert_eq!((unused.locals, unused.writes), (0, 0));
}

#[test]
fn computed_byte_addresses_wrap_only_at_the_authored_operation() {
    for (wrap_base, input, expected) in [
        (false, 0, Some(184)),
        (true, -1, Some(165)),
        (false, -1, None),
    ] {
        let mut instance = byte_at_offset(wrap_base).instantiate();
        assert_eq!(instance.call::<i32>(input).ok(), expected);
        assert_eq!(&instance.memory("state")[..4], &[0xa5, 0xb8, 0x5a, 0xc3]);
        assert!(instance.callbacks().is_empty());
    }
}

#[test]
fn computed_wide_snapshots_survive_partial_and_adjacent_stores() {
    for (offset, expected) in [
        (
            4,
            [0xa5, 0x5a, 0xc3, 0x3c, 7, 0, 0, 0, 9, 0, 0, 0, 3, 0, 0, 0],
        ),
        (
            8,
            [
                0xa5, 0x5a, 0xc3, 0x3c, 7, 0, 0, 0, 0, 0, 0, 0x80, 9, 0, 0, 0,
            ],
        ),
    ] {
        let mut instance = wide_snapshot(false, offset).instantiate();
        assert_eq!(instance.call::<i64>(4).unwrap(), -9223372036854775801);
        assert_eq!(&instance.memory("state")[..16], expected);
        assert!(instance.callbacks().is_empty());
    }
    for (offset, arguments, expected) in [
        (
            0,
            (4, 4),
            [
                0xa5, 0x5a, 0xc3, 0x3c, 9, 0, 0, 0, 0, 0, 0, 0x80, 3, 0, 0, 0,
            ],
        ),
        (
            8,
            (4, 0),
            [0xa5, 0x5a, 0xc3, 0x3c, 7, 0, 0, 0, 9, 0, 0, 0, 3, 0, 0, 0],
        ),
    ] {
        let mut instance = wide_snapshot(true, offset).instantiate();
        assert_eq!(
            instance.call::<i64>(arguments).unwrap(),
            -9223372036854775801
        );
        assert_eq!(&instance.memory("state")[..16], expected);
        assert!(instance.callbacks().is_empty());
    }
    let mut instance = constant_bases().instantiate();
    assert_eq!(instance.call::<i64>(()).unwrap(), -9223372036854775801);
    assert_eq!(
        &instance.memory("state")[..16],
        &[0xa5, 0x5a, 0xc3, 0x3c, 7, 0, 0, 0, 0, 0, 0, 0x80, 9, 0, 0, 0]
    );
    assert!(instance.callbacks().is_empty());
}

#[test]
fn nested_loads_keep_pointer_and_result_snapshots() {
    for (reuse_pointer, expected) in [(false, 7), (true, 15)] {
        let mut instance = nested_loads(reuse_pointer).instantiate();
        assert_eq!(instance.call::<i32>(()).unwrap(), expected);
        assert_eq!(
            &instance.memory("state")[..16],
            &[0x0c, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c, 9, 0, 0, 0, 5, 0, 0, 0]
        );
        assert!(instance.callbacks().is_empty());
    }
}

#[test]
fn deferred_loads_use_captured_pointers_across_memory_writes() {
    let mut instance = deferred_load_with_captured_pointer().instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 7);
    assert_eq!(&instance.memory("state")[..6], &[0x0c, 0, 0, 0, 0xa5, 0x5a]);
    assert_eq!(
        &instance.memory("other")[..16],
        &[0xa5, 0x5a, 0xc3, 0x3c, 0xa5, 0x5a, 0xc3, 0x3c, 7, 0, 0, 0, 5, 0, 0, 0]
    );
    assert!(instance.callbacks().is_empty());
}

#[test]
fn stores_use_captured_address_values() {
    let mut instance = store_through_snapshot().instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 7);
    assert_eq!(
        &instance.memory("state")[..16],
        &[0x0c, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c, 9, 0, 0, 0, 5, 0, 0, 0]
    );
    assert!(instance.callbacks().is_empty());
}

#[test]
fn shared_addresses_reach_all_memory_operations() {
    let mut instance = shared_address().instantiate();
    assert_eq!(instance.call::<i32>(0).unwrap(), 7);
    assert_eq!(
        &instance.memory("state")[..16],
        &[0xa5, 0x5a, 0xc3, 0x3c, 7, 0, 0, 0, 9, 0, 0, 0, 0x0a, 0, 0, 0]
    );
    assert!(instance.callbacks().is_empty());
}

#[test]
fn disjoint_stores_execute_before_deferred_load_traps() {
    let mut instance = disjoint_load_trap().instantiate();
    assert!(instance.call::<i32>(0).is_err());
    assert_eq!(
        &instance.memory("state")[..8],
        &[9, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c]
    );
    assert!(instance.callbacks().is_empty());
}

#[test]
fn computed_accesses_keep_imported_memories_separate() {
    let mut instance = separate_memories().instantiate();
    assert_eq!(instance.call::<i32>(0).unwrap(), 7);
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
    assert_eq!(&instance.memory("other")[..6], &[9, 0, 0, 0, 0x5a, 0xa5]);
    assert!(instance.callbacks().is_empty());
}

#[test]
fn unused_address_chains_do_not_force_trapping_loads() {
    let mut instance = unused_address_chain().instantiate();
    assert_eq!(instance.call::<i32>(65536).unwrap(), 7);
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
    assert_eq!(&instance.memory("other")[..6], &[9, 0, 0, 0, 0x5a, 0xa5]);
    assert!(instance.callbacks().is_empty());
}
