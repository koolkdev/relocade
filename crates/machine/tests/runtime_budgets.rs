mod support;

use relocade_machine::{Cpu, DispatchEntry, Hook};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap,
};
use std::{
    cell::Cell,
    future::{poll_fn, Future},
    rc::Rc,
    task::{Context, Poll, Waker},
};

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
        let target = cpu.instruction_target();
        drop(cpu.run()); // An unpolled run leaves guest state untouched.
        assert_eq!(state.snapshot(), initial);
        target.set(Some(2));
        assert_eq!(cpu.run().await, ExecutionExit::BudgetExhausted);
        assert_eq!(state.instruction_count(), 4);
        assert_eq!((state.ax(), state.bx(), state.eip()), (2, 2, 0x100));
        assert!(!state.interrupts_enabled());
        cpu.instruction_target()
            .set(Some(cpu.cpu_state().instruction_count().wrapping_add(1)));
        assert_eq!(cpu.run().await, ExecutionExit::BudgetExhausted);
        assert_eq!(state.instruction_count(), 7);
        assert_eq!((state.ax(), state.bx()), (3, 3));

        let interrupt = cpu.interrupt_handle();
        interrupt.request();
        target.set(Some(8));
        assert_eq!(cpu.run().await, ExecutionExit::Interrupted);
        assert_eq!(state.instruction_count(), 10);
        assert!(interrupt.take());
        // The controller explicitly clears its target before unbounded stepping.
        target.set(None);
        assert_eq!(cpu.step(), ExecutionExit::StepComplete);
        assert_eq!(state.instruction_count(), 11);
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
        // The requested target wraps to zero. Checkpoints at MAX and zero must
        // continue; rounding to one stops before the host entry on the third jump.
        cpu.instruction_target().set(Some(0));
        assert_eq!(cpu.run().await, ExecutionExit::BudgetExhausted);
        assert_eq!(state.instruction_count(), 1);
        assert_eq!(state.eip(), 0x106);
    }
}

async fn protected_successor() {
    for compiled in [false, true] {
        let compiler = Compiler::new(ExecutionProfile::Real16);
        let mut cpu = cpu(&compiler, &[0xfb, 0x40, 0xeb, 0xfe], compiled, 0).await;
        cpu.instruction_target()
            .set(Some(cpu.cpu_state().instruction_count().wrapping_add(1)));
        assert_eq!(cpu.run().await, ExecutionExit::BudgetExhausted);
        let state = cpu.cpu_state();
        assert_eq!(
            (state.ax(), state.eip(), state.instruction_count()),
            (1, 0x102, 2)
        );
        assert!(state.interrupts_enabled());
    }
}

async fn pending_service(replace: Option<u64>) {
    let compiler = Compiler::new(ExecutionProfile::Real16);
    // INC AX; JMP 0110. A pending host service then resumes at 0120.
    let mut cpu = cpu(&compiler, &[0x40, 0xeb, 0x0d], false, 0).await;
    cpu.memory()
        .view()
        .write(0x120, &[0x43, 0xeb, 0x0d])
        .unwrap(); // INC BX; JMP 0130.
    cpu.memory()
        .view()
        .write(0x130, &[0x41, 0xeb, 0x0d])
        .unwrap(); // INC CX; JMP 0140.
    let state = cpu.cpu_state();
    let ready = Rc::new(Cell::new(false));
    let starts = Rc::new(Cell::new(0));
    let completions = Rc::new(Cell::new(0));
    cpu.install([
        Hook::host(0x110, {
            let (state, ready, starts, completions) = (
                state.clone(),
                ready.clone(),
                starts.clone(),
                completions.clone(),
            );
            async move || {
                starts.set(starts.get() + 1);
                state.set_eip(0x120);
                poll_fn(|_| {
                    if ready.get() {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
                completions.set(completions.get() + 1);
            }
        }),
        Hook::host(0x140, {
            let exit = cpu.exit_handle();
            async move || exit.request()
        }),
    ])
    .await
    .unwrap();
    let target = cpu.instruction_target();
    target.set(Some(3));
    let mut run = Box::pin(cpu.run());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(run.as_mut().poll(&mut cx).is_pending());
    assert!(run.as_mut().poll(&mut cx).is_pending());
    assert_eq!(state.instruction_count(), 2);
    assert_eq!(starts.get(), 1);
    assert_eq!(completions.get(), 0);
    ready.set(true);
    if let Some(allowance) = replace {
        target.set((allowance != 0).then(|| state.instruction_count().wrapping_add(allowance)));
        assert_eq!(completions.get(), 0);
        assert_eq!(run.await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(
            (state.bx(), state.cx(), state.instruction_count()),
            (1, 1, 6)
        );
    } else {
        assert_eq!(run.await, ExecutionExit::BudgetExhausted);
        assert_eq!(
            (state.bx(), state.cx(), state.instruction_count()),
            (1, 0, 4)
        );
        target.set(None);
        assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
        assert_eq!((state.cx(), state.instruction_count()), (1, 6));
    }
    assert_eq!(starts.get(), 1);
    assert_eq!(completions.get(), 1);
}

async fn pending_and_target_updates() {
    pending_service(None).await;
    pending_service(Some(0)).await;
    pending_service(Some(5)).await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn budgets_report_overshoot_and_wraparound_with_interrupt_priority() {
    pollster::block_on(checkpoints());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn budgets_report_overshoot_and_wraparound_with_interrupt_priority() {
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

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn budgets_wait_for_the_protected_successor() {
    pollster::block_on(protected_successor());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn budgets_wait_for_the_protected_successor() {
    protected_successor().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pending_runs_observe_external_target_updates() {
    pollster::block_on(pending_and_target_updates());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pending_runs_observe_external_target_updates() {
    pending_and_target_updates().await;
}
