#![cfg(feature = "worker")]

mod support;

use relocade_machine::{CodeSnapshot, Cpu, DispatchEntry, Error};
use relocade_x86::{
    BlockError, Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping,
    PhysicalMemoryMap, Segment, SegmentRegister, Segments,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn cpu() -> Cpu {
    let mut segments = Segments::real_mode();
    segments.cs = SegmentRegister::real_mode(Segment::Cs, 0x100);
    support::create_cpu(
        support::configuration(
            Compiler::new(ExecutionProfile::Real16).target(),
            CpuState {
                eip: 0x100,
                segments,
                ..CpuState::default()
            },
            0x10000,
            PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap(),
        )
        .unwrap(),
    )
    .await
    .unwrap()
}

async fn check_worker_compilation() {
    let original_cpu = cpu().await;
    let compiler = original_cpu.compiler_worker();
    drop(original_cpu);

    let error = compiler
        .compile_blocks(vec![CodeSnapshot {
            start_eip: 0x100,
            bytes: vec![0x40],
            instruction_limit: 0,
        }])
        .await
        .err()
        .expect("invalid snapshots fail on the worker");
    assert!(matches!(
        error,
        Error::Compile(BlockError::ZeroInstructionLimit)
    ));

    // Both blocks share dispatch. The last byte is an unused, truncated opcode.
    let first = [0x40, 0xeb, 0x0d, 0x0f]; // INC AX; JMP 0110
    let second = [0x43, 0xeb, 0xed]; // INC BX; JMP 0100
    let compiled = compiler
        .compile_blocks(vec![
            CodeSnapshot {
                start_eip: 0x100,
                bytes: first.to_vec(),
                instruction_limit: 1,
            },
            CodeSnapshot {
                start_eip: 0x110,
                bytes: second.to_vec(),
                instruction_limit: 10,
            },
        ])
        .await
        .unwrap();
    assert_eq!(compiled.entries().len(), 2);
    assert_eq!(compiled.entries()[0].start_eip, 0x100);
    assert_eq!(compiled.entries()[0].byte_len, 1);
    assert_eq!(compiled.entries()[1].start_eip, 0x110);
    assert_eq!(compiled.entries()[1].byte_len, 3);
    assert_eq!(
        compiled.execution_target().profile,
        ExecutionProfile::Real16
    );

    let mut cpu = cpu().await;
    cpu.memory().view().write(0x1100, &first).unwrap();
    cpu.memory().view().write(0x1110, &second).unwrap();
    let entries: Vec<_> = compiled
        .entries()
        .iter()
        .map(|entry| DispatchEntry {
            address: 0x1000 + entry.start_eip,
            export: &entry.export,
        })
        .collect();
    cpu.install_compiled_module(compiled.module(), &entries)
        .await
        .unwrap();
    // Only the compiled entry stops after INC. A dispatch miss would interpret
    // INC and JMP together, overshooting this budget and reaching 0110.
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(cpu.cpu_state().instruction_count(), 1);
    assert_eq!(cpu.cpu_state().eip(), 0x101);
    assert_eq!(cpu.run(3).await, ExecutionExit::BudgetExhausted);
    assert_eq!(cpu.cpu_state().ax(), 1);
    assert_eq!(cpu.cpu_state().bx(), 1);
    assert_eq!(cpu.cpu_state().eip(), 0x100);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn worker_compiles_owned_snapshots_for_an_independent_cpu() {
    pollster::block_on(check_worker_compilation());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn worker_compiles_owned_snapshots_for_an_independent_cpu() {
    check_worker_compilation().await;
}
