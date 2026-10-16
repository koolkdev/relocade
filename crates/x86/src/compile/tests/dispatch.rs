//! Dispatch resolves addresses to one function signature in either execution mode.

use super::*;
use crate::{CpuState, EntryTable, ExecutionProfile, Segment, Segments};
use serde::Serialize;
use wasm86_compiler::{Program, Signature, Type};
use wasm86_test_support::Module;

#[derive(Serialize)]
struct Invocation {
    cpu: Vec<u8>,
    entries: Vec<u8>,
    observe: bool,
    export: &'static str,
}

fn cases(profile: ExecutionProfile) -> (Vec<Invocation>, Vec<u64>) {
    let mut cpu = CpuState::default();
    if profile == ExecutionProfile::Real16 {
        cpu.segments = Segments::real_mode();
        cpu.segments[Segment::Cs] = crate::StoredSegment::real_mode(Segment::Cs, 0xffff);
    }
    if profile == SegmentProfile::Segmented16.into() {
        cpu.segments.cs.attributes = crate::SegmentAttributes::from_bits(7);
    }
    let base = cpu.segments[Segment::Cs].base;
    let mut table = EntryTable::default();
    let mut cases = Vec::new();
    let mut expected = Vec::new();
    let mut push = |cpu: CpuState, table: &EntryTable, observe, export, result| {
        assert!(profile.is_compatible_with(&cpu.segments));
        cases.push(Invocation {
            cpu: cpu.to_bytes().to_vec(),
            entries: table.to_bytes(),
            observe,
            export,
        });
        expected.push(result);
    };
    push(cpu, &table, false, "enter", 11);
    table.insert(base, 2);
    push(cpu, &table, false, "enter", 22);
    table.insert(base, 3);
    push(cpu, &table, false, "dispatch", 33);
    if matches!(
        profile,
        ExecutionProfile::Protected(SegmentProfile::Segmented16 | SegmentProfile::Segmented32)
    ) {
        let mut changed = cpu;
        changed.segments.ds.base = 0x4000;
        push(changed, &table, false, "enter", 33);
        // Lookup must still check CS fetch permissions and limits within a valid profile.
        changed = cpu;
        changed.eip = 1;
        changed.segments.cs.limit = 0;
        table.insert(base + 1, 3);
        push(changed, &table, false, "enter", 11);
        changed = cpu;
        changed.segments.cs.attributes =
            crate::SegmentAttributes::from_bits(changed.segments.cs.attributes.bits() & !1);
        push(changed, &table, false, "enter", 11);
    }
    if profile == SegmentProfile::Flat32.into() {
        let mut changed = cpu;
        changed.segments.ds.attributes = crate::SegmentAttributes::from_bits(0xffe5);
        changed.segments.ds.selector = 0x23;
        changed.segments.fs = crate::StoredSegment::unusable(0);
        push(changed, &table, false, "enter", 33);
    }
    push(cpu, &table, true, "dispatch", 0);
    push(cpu, &table, false, "interpret", 11);
    table.remove(base);
    push(cpu, &table, false, "enter", 11);
    // Dense aliases exercise collisions and grow the metadata beyond one Wasm page.
    for offset in 0..8192 {
        table.insert(base.wrapping_add(offset), 2 + offset % 2);
    }
    for offset in [0, 1, 4095, 8191, 8192] {
        cpu.eip = offset;
        push(
            cpu,
            &table,
            false,
            "enter",
            match offset {
                0 => 22,
                8192 => 11,
                _ => 33,
            },
        );
    }
    if profile == ExecutionProfile::Real16 {
        cpu.eip = 0x10000;
        table.insert(base + 0x10000, 2);
        // A registered address cannot bypass the CPU's CS admission check.
        push(cpu, &table, false, "enter", 11);
    } else {
        cpu.eip = u32::MAX;
        table.insert(u32::MAX, 3);
        push(cpu, &table, false, "enter", 33);
    }
    (cases, expected)
}

fn modules(profile: ExecutionProfile) -> (Module, Module) {
    let dispatcher = Compiler::new(profile).compile_dispatcher().unwrap();
    let mut program = Program::new();
    for (name, result) in [("fallback", 11u64), ("first", 22), ("second", 33)] {
        let function = program
            .function(
                Signature {
                    parameters: vec![],
                    results: vec![Type::I64],
                },
                |body| body.return_(result),
            )
            .unwrap();
        program.export(name, function).unwrap();
    }
    (
        Module::new(&dispatcher.bytes),
        Module::new(&program.compile().unwrap()),
    )
}

fn wasmtime(dispatcher: &Module, targets: &Module, cases: &[Invocation]) -> Vec<u64> {
    use ::wasmtime::{Linker, Memory, MemoryType, Ref, RefType, Store, Table, TableType};
    let engine = wasm86_test_support::engine();
    let mut store = Store::new(engine, ());
    let mut linker = Linker::new(engine);
    let cpu = Memory::new(&mut store, MemoryType::new(1, None)).unwrap();
    let entries = Memory::new(&mut store, MemoryType::new(1, None)).unwrap();
    for (name, memory) in [("executionContext", cpu), ("executionEntries", entries)] {
        linker.define(&store, "wasm86", name, memory).unwrap();
    }
    let functions = Table::new(
        &mut store,
        TableType::new(RefType::FUNCREF, 4, None),
        Ref::Func(None),
    )
    .unwrap();
    linker
        .define(&store, "wasm86", "executionFunctions", functions)
        .unwrap();
    let instance = linker
        .instantiate(&mut store, dispatcher.wasmtime())
        .unwrap();
    let callees = linker.instantiate(&mut store, targets.wasmtime()).unwrap();
    let enter = instance.get_func(&mut store, "enter").unwrap();
    functions
        .set(&mut store, 0, Ref::Func(Some(enter)))
        .unwrap();
    for (index, name) in ["fallback", "first", "second"].iter().enumerate() {
        let function = callees.get_func(&mut store, name).unwrap();
        functions
            .set(&mut store, index as u64 + 1, Ref::Func(Some(function)))
            .unwrap();
    }
    cases
        .iter()
        .map(|case| {
            let pages =
                (case.entries.len() as u64 + u64::from(EntryTable::DATA_OFFSET)).div_ceil(65536);
            let current = entries.size(&store);
            if pages > current {
                entries.grow(&mut store, pages - current).unwrap();
            }
            cpu.write(&mut store, 0, &case.cpu).unwrap();
            entries
                .write(&mut store, EntryTable::DATA_OFFSET as usize, &case.entries)
                .unwrap();
            entries
                .write(
                    &mut store,
                    EntryTable::OBSERVE_OFFSET as usize,
                    &u32::from(case.observe).to_le_bytes(),
                )
                .unwrap();
            if case.export == "dispatch" {
                instance
                    .get_typed_func::<i32, i64>(&mut store, "dispatch")
                    .unwrap()
                    .call(&mut store, 0)
                    .unwrap() as u64
            } else {
                instance
                    .get_typed_func::<(), i64>(&mut store, case.export)
                    .unwrap()
                    .call(&mut store, ())
                    .unwrap() as u64
            }
        })
        .collect()
}

#[test]
fn callable_dispatch_resolves_functions_and_fallback() {
    for profile in [
        ExecutionProfile::Real16,
        SegmentProfile::Flat32.into(),
        SegmentProfile::Segmented32.into(),
        SegmentProfile::Segmented16.into(),
    ] {
        let (dispatcher, targets) = modules(profile);
        let (cases, expected) = cases(profile);
        assert_eq!(wasmtime(&dispatcher, &targets, &cases), expected);
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_callable_dispatch_resolves_functions_and_fallback() {
    for profile in [
        ExecutionProfile::Real16,
        SegmentProfile::Flat32.into(),
        SegmentProfile::Segmented32.into(),
        SegmentProfile::Segmented16.into(),
    ] {
        let (dispatcher, targets) = modules(profile);
        let (cases, expected) = cases(profile);
        let results: Vec<String> = wasm86_test_support::run_v8(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/dispatch.mjs"),
            &[&dispatcher, &targets],
            &(EntryTable::DATA_OFFSET, EntryTable::OBSERVE_OFFSET, cases),
        );
        assert_eq!(
            results,
            expected.iter().map(u64::to_string).collect::<Vec<_>>()
        );
    }
}
