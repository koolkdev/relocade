//! Callable entries share address lookup across execution profiles.

use super::*;
use crate::{CpuState, EntryTable, ExecutionContext, ExecutionProfile, Segment, Segments};
use relocade_compiler::{Program, Signature, Type};
use relocade_test_support::Module;

struct Invocation {
    cpu: Vec<u8>,
    entries: Vec<u8>,
    export: &'static str,
}

struct Output {
    word: u64,
    cpu: Vec<u8>,
}

fn cases(profile: ExecutionProfile) -> (Vec<Invocation>, Vec<u64>) {
    let mut cpu = CpuState::default();
    if profile == ExecutionProfile::Real16 {
        cpu.segments = Segments::real_mode();
        cpu.segments[Segment::Cs] = crate::StoredSegment::real_mode(Segment::Cs, 0xffff);
    }
    if profile == SegmentProfile::Segmented16.into() {
        cpu.segments.cs.big = 0;
    }
    if matches!(
        profile,
        ExecutionProfile::Protected(SegmentProfile::Segmented16 | SegmentProfile::Segmented32)
    ) {
        cpu.segments.cs.base = 0xffff_f000;
    }
    let base = cpu.segments[Segment::Cs].base;
    let mut table = EntryTable::default();
    let mut cases = Vec::new();
    let mut expected = Vec::new();
    let mut push = |cpu: CpuState, table: &EntryTable, export, result| {
        assert!(profile.is_compatible_with(&cpu.segments));
        cases.push(Invocation {
            cpu: cpu.to_bytes().to_vec(),
            entries: table.to_bytes(),
            export,
        });
        expected.push(result);
    };
    push(cpu, &table, "dispatch", 11);
    table.insert(base, 2);
    push(cpu, &table, "dispatch", 22);
    table.insert(base, 3);
    push(cpu, &table, "dispatch", 33);
    if matches!(
        profile,
        ExecutionProfile::Protected(SegmentProfile::Segmented16 | SegmentProfile::Segmented32)
    ) {
        let mut changed = cpu;
        changed.segments.ds.base = 0x4000;
        push(changed, &table, "dispatch", 33);
        // Entry requirements belong to the callee, not address lookup.
        changed = cpu;
        changed.eip = 1;
        changed.segments.cs.limit = 0;
        changed.segments.cs.usable = 0;
        table.insert(base.wrapping_add(1), 3);
        push(changed, &table, "dispatch", 33);
    }
    if profile == SegmentProfile::Flat32.into() {
        let mut changed = cpu;
        changed.segments.ds.big = 0;
        changed.segments.ds.selector = 0x23;
        changed.segments.fs = crate::StoredSegment::unusable(0);
        push(changed, &table, "dispatch", 33);
    }
    push(cpu, &table, "interpret", 11);
    table.remove(base);
    push(cpu, &table, "dispatch", 11);
    // Dense aliases exercise collisions and grow the metadata beyond one Wasm page.
    for offset in 0..8192 {
        table.insert(base.wrapping_add(offset), 2 + offset % 2);
    }
    for offset in [0, 1, 4095, 8191, 8192] {
        cpu.eip = offset;
        push(
            cpu,
            &table,
            "dispatch",
            match offset {
                0 => 22,
                8192 => 11,
                _ => 33,
            },
        );
    }
    if profile != ExecutionProfile::Real16 {
        cpu.eip = u32::MAX;
        table.insert(base.wrapping_add(cpu.eip), 3);
        push(cpu, &table, "dispatch", 33);
    }
    (cases, expected)
}

fn constant(result: u64) -> (Module, String) {
    let mut program = Program::new();
    let function = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I64],
            },
            |body| body.return_(result),
        )
        .unwrap();
    program.export("dispatch", function).unwrap();
    (Module::new(&program.compile().unwrap()), "dispatch".into())
}

fn execute(dispatcher: &Module, targets: &[(Module, String)], cases: &[Invocation]) -> Vec<Output> {
    use ::wasmtime::{Linker, Memory, MemoryType, Ref, RefType, Store, Table, TableType};
    let engine = relocade_test_support::engine();
    let mut store = Store::new(engine, ());
    let mut linker = Linker::new(engine);
    let cpu = Memory::new(&mut store, MemoryType::new(1, None)).unwrap();
    let entries = Memory::new(&mut store, MemoryType::new(1, None)).unwrap();
    for (name, memory) in [("executionContext", cpu), ("executionEntries", entries)] {
        linker.define(&store, "relocade", name, memory).unwrap();
    }
    let functions = Table::new(
        &mut store,
        TableType::new(RefType::FUNCREF, targets.len() as u32 + 1, None),
        Ref::Func(None),
    )
    .unwrap();
    linker
        .define(&store, "relocade", "executionFunctions", functions)
        .unwrap();
    let instance = linker
        .instantiate(&mut store, dispatcher.wasmtime())
        .unwrap();
    for name in ["dispatch", "interpret"] {
        let function = instance.get_func(&mut store, name).unwrap();
        linker.define(&store, "relocade", name, function).unwrap();
    }
    for (index, (module, name)) in targets.iter().enumerate() {
        let target = linker.instantiate(&mut store, module.wasmtime()).unwrap();
        let function = target.get_func(&mut store, name).unwrap();
        functions
            .set(&mut store, index as u64 + 1, Ref::Func(Some(function)))
            .unwrap();
    }
    cases
        .iter()
        .map(|case| {
            let pages = (case.entries.len() as u64).div_ceil(65536);
            let current = entries.size(&store);
            if pages > current {
                entries.grow(&mut store, pages - current).unwrap();
            }
            cpu.write(&mut store, 0, &case.cpu).unwrap();
            entries.write(&mut store, 0, &case.entries).unwrap();
            let word = instance
                .get_typed_func::<(), i64>(&mut store, case.export)
                .unwrap()
                .call(&mut store, ())
                .unwrap() as u64;
            Output {
                word,
                cpu: cpu.data(&store)[..case.cpu.len()].to_vec(),
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
        let dispatcher = Module::new(&Compiler::new(profile).compile_dispatcher().unwrap().bytes);
        let targets = [constant(11), constant(22), constant(33)];
        let (cases, expected) = cases(profile);
        let outputs = execute(&dispatcher, &targets, &cases);
        assert_eq!(
            outputs.iter().map(|output| output.word).collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn compiled_blocks_chain_until_budget_or_missing_entry() {
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let dispatcher = Module::new(&compiler.compile_dispatcher().unwrap().bytes);
    let mut targets = vec![constant(11)];
    let mut table = EntryTable::default();
    // INC EAX; JMP 0x2000, followed by INC EBX; JMP 0x1000.
    for (address, code) in [
        (0x1000, [0x40, 0xe9, 0xfa, 0x0f, 0, 0]),
        (0x2000, [0x43, 0xe9, 0xfa, 0xef, 0xff, 0xff]),
    ] {
        let block = compiler.compile_block(address, &code, 2).unwrap();
        table.insert(address, targets.len() as u32 + 1);
        targets.push((Module::new(&block.bytes), block.entry));
    }
    let mut context = ExecutionContext::default();
    context.cpu.eip = 0x1000;
    context.instruction_target = 5;
    let mut cases = vec![Invocation {
        cpu: context.to_bytes().to_vec(),
        entries: table.to_bytes(),
        export: "dispatch",
    }];
    // The same first block reaches fallback when its successor has no mapping.
    table.remove(0x2000);
    cases.push(Invocation {
        cpu: context.to_bytes().to_vec(),
        entries: table.to_bytes(),
        export: "dispatch",
    });
    let outputs = execute(&dispatcher, &targets, &cases);
    for (output, (word, count, eax, ebx)) in outputs.iter().zip([(1 << 63, 6, 2, 1), (11, 2, 1, 0)])
    {
        assert_eq!(output.word, word);
        let cpu = CpuState::from_bytes(output.cpu[..CpuState::BYTE_LEN].try_into().unwrap());
        assert_eq!(
            (
                cpu.eip,
                cpu.instruction_count,
                cpu.registers.eax,
                cpu.registers.ebx
            ),
            (0x2000, count, eax, ebx)
        );
    }
}
