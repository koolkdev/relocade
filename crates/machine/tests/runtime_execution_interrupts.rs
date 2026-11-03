mod support;

use relocade_machine::{Cpu, DispatchEntry, IoBus, Memory};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn cpu(code: &[u8], compiled: bool, io: IoBus) -> Cpu {
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let config = support::configuration(
        compiler.target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let memory = Memory::new(config.machine).unwrap();
    memory.view().write(0x100, code).unwrap();
    let mut cpu = Cpu::new(memory, config.cpu, io).await.unwrap();
    if compiled {
        let module = compiler.compile_block(0x100, code, 32).unwrap();
        cpu.install_module(
            &module.bytes,
            &[DispatchEntry {
                address: 0x100,
                export: &module.entry,
            }],
        )
        .await
        .unwrap();
    }
    cpu
}

async fn checkpoints() {
    for compiled in [false, true] {
        // INC AX; INC BX; JMP 0100. IF stays clear throughout.
        let mut cpu = cpu(&[0x40, 0x43, 0xeb, 0xfc], compiled, IoBus::default()).await;
        let state = cpu.cpu_state();
        let saved = state.snapshot();
        let interrupt = cpu.interrupt_handle();
        interrupt.request();
        interrupt.request();
        state.restore(&saved).unwrap();
        assert_eq!(cpu.run(0).await, ExecutionExit::BudgetExhausted);
        assert_eq!(state.snapshot(), saved);
        for iteration in 1..=2 {
            assert_eq!(cpu.run(100).await, ExecutionExit::Interrupted);
            assert_eq!(state.ax(), iteration);
            assert_eq!(state.bx(), iteration);
            assert_eq!(state.eip(), 0x100);
            assert_eq!(state.instruction_count(), u64::from(iteration) * 3);
            assert!(!state.interrupts_enabled());
        }
        assert!(interrupt.take());
        assert!(!interrupt.take());
        assert_eq!(cpu.run(3).await, ExecutionExit::BudgetExhausted);
        assert_eq!(state.ax(), 3);
        // Another request also survives a coincident budget boundary.
        interrupt.request();
        assert_eq!(cpu.run(3).await, ExecutionExit::Interrupted);
        assert_eq!(state.ax(), 4);
        drop(cpu);
        assert!(interrupt.take());
    }
}

async fn protected_step() {
    for compiled in [false, true] {
        // STI; INC AX; JMP 0102. The protected INC must complete before exit.
        let mut cpu = cpu(&[0xfb, 0x40, 0xeb, 0xfe], compiled, IoBus::default()).await;
        let interrupt = cpu.interrupt_handle();
        interrupt.request();
        assert_eq!(cpu.run(1).await, ExecutionExit::Interrupted);
        let state = cpu.cpu_state();
        assert!(state.interrupts_enabled());
        assert_eq!(state.ax(), 1);
        assert_eq!(state.eip(), 0x102);
        assert_eq!(state.instruction_count(), 2);
        assert!(interrupt.take());
        assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
        assert_eq!(state.instruction_count(), 3);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn checkpoints_preserve_requests_and_publish_cpu_state() {
    pollster::block_on(checkpoints());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn checkpoints_preserve_requests_and_publish_cpu_state() {
    checkpoints().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn execution_interrupt_waits_for_the_protected_successor() {
    pollster::block_on(protected_step());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn execution_interrupt_waits_for_the_protected_successor() {
    protected_step().await;
}

#[cfg(feature = "worker")]
#[path = "runtime_execution_interrupts/worker.rs"]
mod worker;

#[cfg(feature = "worker")]
#[path = "runtime_execution_interrupts/session.rs"]
mod session;
