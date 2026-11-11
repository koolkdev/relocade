#[path = "support/compilation.rs"]
mod compilation;

mod support;

use relocade_machine::{Cpu, DispatchEntry, IoBus, Memory, Runner};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};

struct Stopped {
    state: relocade_machine::CpuStateAccess,
    report: Option<futures_channel::oneshot::Sender<u64>>,
}

impl Drop for Stopped {
    fn drop(&mut self) {
        let _ = self
            .report
            .take()
            .unwrap()
            .send(self.state.instruction_count());
    }
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn configuration() -> support::Configuration {
    support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap()
}

async fn cpu(code: &[u8], compiled: bool, io: IoBus) -> Cpu {
    let config = configuration();
    let memory = Memory::new(config.machine).unwrap();
    let mut cpu = Cpu::new(memory, config.cpu, io).await.unwrap();
    install(&mut cpu, code, compiled).await;
    cpu
}

async fn prepare_cpu(runner: &mut Runner, code: &[u8], compiled: bool, io: IoBus) {
    let cpu = runner.attach_io(io);
    install(cpu, code, compiled).await;
}

async fn install(cpu: &mut Cpu, code: &[u8], compiled: bool) {
    cpu.memory().view().write(0x100, code).unwrap();
    if compiled {
        let module = Compiler::new(ExecutionProfile::Real16)
            .compile_block(0x100, code, 32)
            .unwrap();
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
        drop(cpu.run());
        assert_eq!(state.snapshot(), saved);
        for iteration in 1..=2 {
            assert_eq!(cpu.run().await, ExecutionExit::Interrupted);
            assert_eq!(state.ax(), iteration);
            assert_eq!(state.bx(), iteration);
            assert_eq!(state.eip(), 0x100);
            assert_eq!(state.instruction_count(), u64::from(iteration) * 3);
            assert!(!state.interrupts_enabled());
        }
        assert!(interrupt.take());
        assert!(!interrupt.take());
        support::steps(&mut cpu, 3);
        assert_eq!(state.ax(), 3);
        // A fresh request returns through the same compiled or interpreted boundary.
        interrupt.request();
        assert_eq!(cpu.run().await, ExecutionExit::Interrupted);
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
        assert_eq!(cpu.run().await, ExecutionExit::Interrupted);
        let state = cpu.cpu_state();
        assert!(state.interrupts_enabled());
        assert_eq!(state.ax(), 1);
        assert_eq!(state.eip(), 0x102);
        assert_eq!(state.instruction_count(), 2);
        assert!(interrupt.take());
        support::steps(&mut cpu, 1);
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

#[path = "runtime_execution_interrupts/worker.rs"]
mod worker;

#[path = "runtime_execution_interrupts/construction.rs"]
mod construction;

#[path = "runtime_execution_interrupts/session.rs"]
mod session;

#[path = "runtime_execution_interrupts/halt.rs"]
mod halt;
