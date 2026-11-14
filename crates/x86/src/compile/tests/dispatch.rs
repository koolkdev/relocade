//! Real-mode callable entries resolve by packed CS:IP.

use super::*;
use crate::{CpuState, EntryTable, ExecutionContext, ExecutionProfile, Segment};
use relocade_compiler::{
    abi::{
        slots::{Layout, Tuple},
        AbiValue,
    },
    Program,
};
use relocade_test_support::Module;

#[test]
fn far_call_dispatch_respects_execution_profile() {
    for profile in [ExecutionProfile::Real16, SegmentProfile::Segmented16.into()] {
        let block = Compiler::new(profile)
            .compile_block(0x100, &[0x9a, 0, 2, 0x10, 0], 1)
            .unwrap();
        let module = Module::new(&block.bytes);
        let resolves_call = module
            .wasmtime()
            .imports()
            .any(|import| import.module() == "relocade" && import.name() == "resolveCall");
        assert_eq!(resolves_call, profile == ExecutionProfile::Real16);
    }
}

struct Invocation {
    cpu: Vec<u8>,
    entries: Vec<u8>,
    export: &'static str,
}

struct Output {
    exit: crate::ExecutionExit,
    cpu: Vec<u8>,
}

fn cases() -> (Vec<Invocation>, Vec<crate::ExecutionExit>) {
    let mut cpu = CpuState::real_mode();
    cpu.segments.cs = crate::SegmentRegister::real_mode(Segment::Cs, 0xffff);
    let key = 0xffff_0000;
    let mut table = EntryTable::default();
    let mut cases = Vec::new();
    let mut expected = Vec::new();
    let miss = crate::ExecutionExit::Host(crate::HostExit::Return);
    let first = crate::ExecutionExit::Host(crate::HostExit::Call {
        service_id: 0x89ab_cdef,
    });
    let replacement = crate::ExecutionExit::UnsupportedInstruction {
        eip: u32::MAX,
        opcode: 0xf3,
    };
    let mut push = |cpu: CpuState, table: &EntryTable, export, result| {
        cases.push(Invocation {
            cpu: cpu.to_bytes().to_vec(),
            entries: table.to_bytes(),
            export,
        });
        expected.push(result);
    };
    push(cpu, &table, "dispatch", miss);
    table.insert(key, 2);
    push(cpu, &table, "dispatch", first);
    table.insert(key, 3);
    push(cpu, &table, "dispatch", replacement);
    push(cpu, &table, "interpret", miss);

    // The same linear address under another CS has a distinct entry.
    let mut alias = cpu;
    alias.segments.cs = crate::SegmentRegister::real_mode(Segment::Cs, 0xfffe);
    alias.eip = 0x10;
    push(alias, &table, "dispatch", miss);
    table.insert(0xfffe_0010, 2);
    push(alias, &table, "dispatch", first);
    let mut invalid_ip = cpu;
    invalid_ip.eip = 0x10000;
    push(invalid_ip, &table, "dispatch", miss);

    table.remove(key);
    push(cpu, &table, "dispatch", miss);
    // Dense keys exercise collisions and grow the metadata beyond one Wasm page.
    for offset in 0..8192 {
        table.insert(key | offset, 2 + offset % 2);
    }
    for offset in [0, 1, 4095, 8191, 8192] {
        cpu.eip = offset;
        push(
            cpu,
            &table,
            "dispatch",
            if offset == 8192 {
                miss
            } else if offset % 2 == 0 {
                first
            } else {
                replacement
            },
        );
    }
    (cases, expected)
}

fn constant(result: crate::ExecutionExit) -> (Module, String) {
    let mut program = Program::new();
    let function = program.function::<crate::abi::ExecutionSignature>(move |body, ()| {
        body.return_(crate::ExecutionExitValues::from(result))
    });
    program.export("dispatch", function).unwrap();
    (Module::new(&program.compile().unwrap()), "dispatch".into())
}

fn execute(dispatcher: &Module, targets: &[(Module, String)], cases: &[Invocation]) -> Vec<Output> {
    use ::wasmtime::{Linker, Memory, MemoryType, Ref, RefType, Store, Table, TableType};
    let engine = relocade_test_support::engine();
    let mut store = Store::new(engine, ());
    let mut linker = Linker::new(engine);
    let cpu = relocade_test_support::SharedBytes::new(1, 1);
    let entries = Memory::new(&mut store, MemoryType::new(1, None)).unwrap();
    linker
        .define(&store, "relocade", "executionContext", cpu.memory().clone())
        .unwrap();
    linker
        .define(&store, "relocade", "executionEntries", entries)
        .unwrap();
    let call_profile = Memory::new(&mut store, MemoryType::new(1, None)).unwrap();
    linker
        .define(&store, "relocade", "callProfile", call_profile)
        .unwrap();
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
    for name in ["dispatch", "interpret", "resolveCall"] {
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
            cpu.write(0, &case.cpu);
            entries.write(&mut store, 0, &case.entries).unwrap();
            let values = instance
                .get_typed_func::<(), <Layout<crate::ExecutionExit> as Tuple>::Tuple>(
                    &mut store,
                    case.export,
                )
                .unwrap()
                .call(&mut store, ())
                .unwrap();
            Output {
                exit: crate::ExecutionExit::lift(Layout::<crate::ExecutionExit>::from_tuple(
                    values,
                ))
                .unwrap(),
                cpu: cpu.read(0, case.cpu.len()),
            }
        })
        .collect()
}

#[test]
fn callable_dispatch_resolves_functions_and_fallback() {
    let dispatcher = Module::new(
        &Compiler::new(ExecutionProfile::Real16)
            .compile_dispatcher()
            .unwrap()
            .bytes,
    );
    let targets = [
        constant(crate::ExecutionExit::Host(crate::HostExit::Return)),
        constant(crate::ExecutionExit::Host(crate::HostExit::Call {
            service_id: 0x89ab_cdef,
        })),
        constant(crate::ExecutionExit::UnsupportedInstruction {
            eip: u32::MAX,
            opcode: 0xf3,
        }),
    ];
    let (cases, expected) = cases();
    let outputs = execute(&dispatcher, &targets, &cases);
    assert_eq!(
        outputs.iter().map(|output| output.exit).collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn compiled_blocks_chain_until_interrupt_or_missing_entry() {
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let dispatcher = Module::new(&compiler.compile_dispatcher().unwrap().bytes);
    let mut targets = vec![constant(crate::ExecutionExit::Host(
        crate::HostExit::Return,
    ))];
    let mut table = EntryTable::default();
    // INC AX; JMP 0x2000, followed by INC BX; JMP 0x3000.
    for (address, code) in [
        (0x1000, [0x40, 0xe9, 0xfc, 0x0f]),
        (0x2000, [0x43, 0xe9, 0xfc, 0x0f]),
    ] {
        let block = compiler.compile_block(address, &code, 2).unwrap();
        table.insert(address, targets.len() as u32 + 1);
        targets.push((Module::new(&block.bytes), block.entry));
    }
    let mut context = ExecutionContext {
        cpu: CpuState::real_mode(),
        ..ExecutionContext::default()
    };
    context.cpu.eip = 0x1000;
    let mut cases = vec![Invocation {
        cpu: context.to_bytes().to_vec(),
        entries: table.to_bytes(),
        export: "dispatch",
    }];
    context.interrupt_requested = 1;
    cases.push(Invocation {
        cpu: context.to_bytes().to_vec(),
        entries: table.to_bytes(),
        export: "dispatch",
    });
    context.interrupt_requested = 0;
    // The same first block reaches fallback when its successor has no mapping.
    table.remove(0x2000);
    cases.push(Invocation {
        cpu: context.to_bytes().to_vec(),
        entries: table.to_bytes(),
        export: "dispatch",
    });
    let outputs = execute(&dispatcher, &targets, &cases);
    for (output, (exit, eip, count, eax, ebx)) in outputs.iter().zip([
        (
            crate::ExecutionExit::Host(crate::HostExit::Return),
            0x3000,
            4,
            1,
            1,
        ),
        (crate::ExecutionExit::Interrupted, 0x2000, 2, 1, 0),
        (
            crate::ExecutionExit::Host(crate::HostExit::Return),
            0x2000,
            2,
            1,
            0,
        ),
    ]) {
        assert_eq!(output.exit, exit);
        let cpu = CpuState::from_bytes(output.cpu[..CpuState::BYTE_LEN].try_into().unwrap());
        assert_eq!(
            (
                cpu.eip,
                cpu.instruction_count,
                cpu.registers.eax,
                cpu.registers.ebx
            ),
            (eip, count, eax, ebx)
        );
    }
}
