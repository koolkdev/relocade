use super::*;
use crate::{Cpu, CpuConfig, DispatchEntry, Hook, MachineConfig, Memory};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap,
};

#[test]
fn threshold_crossings_queue_once_without_scanning_cold_entries() {
    let mut hotness = Hotness::default();
    hotness.record(CallCount {
        target: 1,
        count: 1023,
    });
    assert_eq!(hotness.next_candidate(), None);
    hotness.record(CallCount {
        target: 2,
        count: 1024,
    });
    hotness.record(CallCount {
        target: 1,
        count: 10,
    });
    assert_eq!(hotness.next_candidate(), Some(2));
    assert_eq!(hotness.next_candidate(), Some(1));
    hotness.record(CallCount {
        target: 2,
        count: 10000,
    });
    assert_eq!(hotness.next_candidate(), None);
    assert_eq!(hotness.count(1), 1033);
    assert_eq!(hotness.count(2), 11024);
}

async fn replace_fallback() {
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let memory = Memory::new(
        MachineConfig::new(
            0x10000,
            PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    memory.view().write(0x200, &[0x40, 0xc3]).unwrap(); // INC AX; RET
    memory.view().write(0x300, &[0x42, 0xe9, 0xfc, 0]).unwrap(); // INC DX; JMP 0400
    memory.view().write(0x7fe, &0x400u16.to_le_bytes()).unwrap();
    let mut state = CpuState {
        eip: 0x300,
        ..CpuState::real_mode()
    };
    state.registers.esp = 0x7fe;
    let mut cpu = Cpu::new(
        memory,
        CpuConfig::new(compiler.target(), state).unwrap(),
        Default::default(),
    )
    .await
    .unwrap();
    let exit = cpu.exit_handle();
    cpu.install([Hook::host(0x400, async move || exit.request())])
        .await
        .unwrap();
    cpu.interpret_at(0x200).unwrap();
    cpu.interpret_at(0x300).unwrap();
    let block = compiler.compile_block(0x200, &[0x40, 0xc3], 2).unwrap();
    cpu.install_module(
        &block.bytes,
        &[DispatchEntry {
            address: 0x200,
            export: &block.entry,
        }],
    )
    .await
    .unwrap();
    // Replacing one fallback must allocate a dynamic slot, leaving the shared
    // interpreter slot and the other rejected candidate untouched.
    assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
    assert_eq!((cpu.cpu_state().ax(), cpu.cpu_state().dx()), (0, 1));
    cpu.cpu_state().set_eip(0x200);
    assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
    assert_eq!((cpu.cpu_state().ax(), cpu.cpu_state().dx()), (1, 1));
    assert_eq!(cpu.cpu_state().sp(), 0x800);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn replacing_a_rejected_candidate_preserves_other_interpreter_entries() {
    pollster::block_on(replace_fallback());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn replacing_a_rejected_candidate_preserves_other_interpreter_entries() {
    replace_fallback().await;
}
