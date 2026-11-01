use super::*;
use relocade_machine::{Cpu, StopHandle};

async fn cpu() -> Cpu {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x400,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let mut cpu = support::create_cpu(config).await.unwrap();
    // Completion changes AX before reaching a budget boundary.
    cpu.memory()
        .view()
        .write(0x401, &[0x40, 0xeb, 0xfe])
        .unwrap();
    cpu.install_hooks([Hook::new(0x400, async || {}).with_continuations(0x500, 0x401)])
        .await
        .unwrap();
    cpu
}

async fn stop_before_poll() {
    let mut runner = cpu().await;
    let stop = runner.stop_handle();
    stop.stop();
    stop.stop();
    drop(runner.run(1));
    assert_eq!(runner.run(0).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runner.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(runner.cpu_state().eip(), 0x400);
    assert_eq!(runner.cpu_state().instruction_count(), 0);
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runner.cpu_state().ax(), 1, "the stop request was consumed");
}

async fn stop_pending_waiter() {
    let mut runner = cpu().await;
    let stop = runner.stop_handle();
    let state = Rc::new(State::default());
    let cpu = runner.cpu_state();
    runner
        .install_hooks([Hook::new(0x400, {
            let (input, cpu) = (state.clone(), cpu.clone());
            async move || {
                let _operation = Operation(input.clone());
                let saved_if = cpu.eflags() & 0x200;
                cpu.set_eflags(cpu.eflags() | 0x200);
                let value = input.read().await;
                cpu.set_eflags((cpu.eflags() & !0x200) | saved_if);
                cpu.set_ax(value);
                input.completed.set(input.completed.get() + 1);
            }
        })
        .with_continuations(0x500, 0x401)])
        .await
        .unwrap();
    let old = Arc::new(WakeCount::default());
    let new = Arc::new(WakeCount::default());
    let old_waker = Waker::from(old.clone());
    let new_waker = Waker::from(new.clone());
    {
        let mut run = pin!(runner.run(1));
        assert!(run
            .as_mut()
            .poll(&mut Context::from_waker(&old_waker))
            .is_pending());
    }
    // Cancelling a waiter unregisters only its wakeup, not retained execution.
    stop.stop();
    assert_eq!(old.0.load(Ordering::Relaxed), 0);
    assert_eq!(state.dropped.get(), 0);
    assert_eq!(runner.run(0).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runner.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(state.dropped.get(), 1);
    {
        let mut run = pin!(runner.run(1));
        assert!(run
            .as_mut()
            .poll(&mut Context::from_waker(&new_waker))
            .is_pending());
        cpu.set_eflags(0x403);
        state.send(&[42]);
        new.0.store(0, Ordering::Relaxed);
        stop.stop();
        assert_eq!(old.0.load(Ordering::Relaxed), 0);
        assert_eq!(new.0.load(Ordering::Relaxed), 1);
    }
    assert_eq!(state.dropped.get(), 1);
    assert_eq!(runner.run(0).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runner.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(
        state.completed.get(),
        0,
        "ready input must not resume a stopped call"
    );
    assert_eq!(state.dropped.get(), 2);
    assert_eq!(cpu.ax(), 0);
    assert_eq!(cpu.eip(), 0x400);
    assert_eq!(
        cpu.eflags(),
        0x403,
        "cancellation must not restore saved IF"
    );
    assert_eq!(cpu.instruction_count(), 0);
}

struct StopOnDrop {
    stop: StopHandle,
    state: Rc<State>,
    pending: bool,
}

impl Future for StopOnDrop {
    type Output = ();

    fn poll(self: std::pin::Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if self.pending {
            self.stop.stop();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.state.dropped.set(self.state.dropped.get() + 1);
        self.stop.stop();
    }
}

async fn stop_during_poll() {
    for pending in [false, true] {
        let mut runner = cpu().await;
        let state = Rc::new(State::default());
        runner
            .install_hooks([Hook::new(0x400, {
                let stop = runner.stop_handle();
                let state = state.clone();
                move || StopOnDrop {
                    stop: stop.clone(),
                    state: state.clone(),
                    pending,
                }
            })
            .with_continuations(0x500, 0x401)])
            .await
            .unwrap();
        assert_eq!(runner.run(1).await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(state.dropped.get(), 1);
        assert_eq!(runner.cpu_state().eip(), 0x400);
        assert_eq!(runner.cpu_state().ax(), 0);
        assert_eq!(runner.cpu_state().instruction_count(), 0);
    }
}

macro_rules! test_backends {
    ($name:ident, $check:ident) => {
        #[cfg(not(target_arch = "wasm32"))]
        #[test]
        fn $name() {
            pollster::block_on($check());
        }
        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen_test::wasm_bindgen_test]
        async fn $name() {
            $check().await;
        }
    };
}

test_backends!(
    stop_requests_survive_zero_budget_and_are_consumed_once,
    stop_before_poll
);
test_backends!(
    stop_wakes_the_current_waiter_and_cancels_without_resumption,
    stop_pending_waiter
);
test_backends!(
    stop_from_pending_or_destroyed_ready_future_skips_completion,
    stop_during_poll
);
