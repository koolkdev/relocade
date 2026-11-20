mod support;

use relocade_machine::{Cpu, DispatchEntry, GuestTimeDriver, Hook};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap,
};
use std::{num::NonZeroU64, task::Waker};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn cpu(compiler: &Compiler, code: &[u8], compiled: bool, count: u64) -> Cpu {
    let config = support::configuration(
        compiler.target(),
        CpuState {
            eip: 0x100,
            instruction_count: count,
            ..CpuState::real_mode()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let mut cpu = support::create_cpu(config).await.unwrap();
    cpu.memory().view().write(0x100, code).unwrap();
    if compiled {
        let module = compiler.compile_block(0x100, code, 32).unwrap();
        cpu.install_module(
            &module.bytes,
            &[DispatchEntry {
                key: 0x100,
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
        let compiler = Compiler::new(ExecutionProfile::Real16);
        // INC AX; INC BX; JMP 0100. Each completed block retires three instructions.
        let mut cpu = cpu(&compiler, &[0x40, 0x43, 0xeb, 0xfc], compiled, u64::MAX - 1).await;
        let state = cpu.cpu_state();
        let initial = state.snapshot();
        let time = GuestTimeDriver::new(NonZeroU64::MIN);
        let _binding = time.bind(&cpu, Waker::noop());
        // The timer at guest tick four crosses the retirement counter wrap.
        let sleep = support::guest_sleep(&time, 4);
        drop(cpu.run());
        assert_eq!(state.snapshot(), initial);
        assert_eq!(cpu.run().await, ExecutionExit::BudgetExhausted);
        assert_eq!(state.instruction_count(), 4);
        assert_eq!((state.ax(), state.bx(), state.eip()), (2, 2, 0x100));
        assert!(!state.interrupts_enabled());
        drop(sleep);

        let _sleep = support::guest_sleep(&time, 7);
        let interrupt = cpu.interrupt_handle();
        interrupt.request();
        assert_eq!(cpu.run().await, ExecutionExit::Interrupted);
        assert_eq!(state.instruction_count(), 7);
        assert!(interrupt.take());
    }
}

async fn unlimited_runs() {
    for budgets in [false, true] {
        let compiler = Compiler::new(ExecutionProfile::Real16).with_instruction_budgets(budgets);
        for compiled in [false, true] {
            // INC AX; JMP 0103; the registered host entry ends execution.
            let mut cpu = cpu(&compiler, &[0x40, 0xeb, 0], compiled, 0).await;
            let state = cpu.cpu_state();
            cpu.install([Hook::host(0x103, {
                let exit = cpu.exit_handle();
                async move || exit.request()
            })])
            .await
            .unwrap();
            assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
            assert_eq!((state.ax(), state.instruction_count()), (1, 2));
            state.set_eip(0x100);
            assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
            assert_eq!((state.ax(), state.instruction_count()), (2, 4));
        }
    }
}

async fn wrapped_zero_target() {
    for compiled in [false, true] {
        let compiler = Compiler::new(ExecutionProfile::Real16);
        // Three JMP-next checkpoints, then a finite host exit if budgeting is disabled.
        let mut cpu = cpu(
            &compiler,
            &[0xeb, 0, 0xeb, 0, 0xeb, 0],
            compiled,
            u64::MAX - 1,
        )
        .await;
        let state = cpu.cpu_state();
        cpu.install([Hook::host(0x106, {
            let exit = cpu.exit_handle();
            async move || exit.request()
        })])
        .await
        .unwrap();
        // Tick two maps to retired count zero, the disabled-target sentinel.
        // The next representable checkpoint is count one, after the third jump.
        let time = GuestTimeDriver::new(NonZeroU64::MIN);
        let _binding = time.bind(&cpu, Waker::noop());
        let _sleep = support::guest_sleep(&time, 2);
        assert_eq!(cpu.run().await, ExecutionExit::BudgetExhausted);
        assert_eq!(state.instruction_count(), 1);
        assert_eq!(state.eip(), 0x106);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn guest_deadlines_handle_block_overshoot_and_counter_wrap_with_interrupt_priority() {
    pollster::block_on(checkpoints());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn guest_deadlines_handle_block_overshoot_and_counter_wrap_with_interrupt_priority() {
    checkpoints().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unarmed_runs_support_both_target_policies() {
    pollster::block_on(unlimited_runs());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn unarmed_runs_support_both_target_policies() {
    unlimited_runs().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_wrapped_zero_target_postpones_exhaustion_without_disabling_it() {
    pollster::block_on(wrapped_zero_target());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_wrapped_zero_target_postpones_exhaustion_without_disabling_it() {
    wrapped_zero_target().await;
}
