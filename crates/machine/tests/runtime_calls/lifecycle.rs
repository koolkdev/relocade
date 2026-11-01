use super::{cpu, pending, Input, Irq, Operation};
use relocade_machine::{real_mode_interrupt, Hook};
use relocade_x86::{ExecutionExit, HostExit};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

async fn redirected_hook() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    // Discard the IRQ frame and jump to the original entry with its original SS:SP.
    runner
        .memory()
        .view()
        .write(0x300, &[0x83, 0xc4, 6, 0xe9, 0xfa, 0xfe])
        .unwrap();
    let input = Rc::new(Input::default());
    let starts = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    let state = runner.cpu_state();
    runner
        .install_hooks([real_mode_interrupt::hook(0x200, {
            let (source, started, dropped, cpu) =
                (input.clone(), starts.clone(), drops.clone(), state.clone());
            let stop = runner.stop_handle();
            async move || {
                let index = started.get();
                started.set(index + 1);
                let _operation = Operation(dropped.clone());
                assert_eq!(cpu.sp(), 0x7ffa);
                if index == 0 {
                    real_mode_interrupt::with_interrupts_enabled(&cpu, source.read()).await;
                    panic!("redirecting to the original hook cannot resume the outer future");
                }
                cpu.set_eflags(0x603);
                stop.stop();
            }
        })])
        .await
        .unwrap();
    pending(runner.run(10));
    irq.borrow_mut().raise();
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
    input.send(7);
    assert_eq!(runner.run(10).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(starts.get(), 2);
    assert_eq!(input.consumed.get(), 0);
    assert_eq!(
        drops.get(),
        2,
        "session return drops the parked outer invocation"
    );
    assert_eq!(
        state.eflags(),
        0x603,
        "abandonment cannot restore the outer call's IF"
    );
}

async fn explicit_abandonment() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    runner
        .memory()
        .view()
        .write(0x300, &[0x83, 0xc4, 6, 0xe9, 0xfa, 0])
        .unwrap(); // Discard frame; JMP 0400.
    runner.memory().view().write(0x400, &[0xeb, 0xfe]).unwrap();
    let input = Rc::new(Input::default());
    let drops = Rc::new(Cell::new(0));
    let state = runner.cpu_state();
    runner
        .install_hooks([real_mode_interrupt::hook(0x200, {
            let (source, dropped, cpu) = (input.clone(), drops.clone(), state.clone());
            async move || {
                let _operation = Operation(dropped.clone());
                real_mode_interrupt::with_interrupts_enabled(&cpu, source.read()).await;
                panic!("abandoned service cannot finish");
            }
        })])
        .await
        .unwrap();
    pending(runner.run(10));
    irq.borrow_mut().raise();
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(state.eip(), 0x400);
    input.send(7);
    assert_eq!(runner.run(10).await, ExecutionExit::BudgetExhausted);
    assert_eq!(
        input.consumed.get(),
        0,
        "redirection alone leaves work parked"
    );
    state.set_interrupts_enabled(true);
    let before = state.snapshot();
    runner.abandon_suspended_calls();
    assert_eq!(drops.get(), 1);
    assert_eq!(state.snapshot(), before);
    // Registration is available again after explicit abandonment.
    runner
        .install_hooks([Hook::new(0x400, {
            let stop = runner.stop_handle();
            async move || stop.stop()
        })])
        .await
        .unwrap();
    assert_eq!(runner.run(1).await, ExecutionExit::Host(HostExit::Return));
    drop(runner);
    assert_eq!(drops.get(), 1);
    state.set_ax(99);
    assert_eq!(
        state.ax(),
        99,
        "resource handles remain valid after runner destruction"
    );
}

async fn destruction_order() {
    struct DropOrder {
        index: usize,
        order: Rc<RefCell<Vec<usize>>>,
    }
    impl Drop for DropOrder {
        fn drop(&mut self) {
            self.order.borrow_mut().push(self.index);
        }
    }
    for stopping in [false, true] {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let mut runner = cpu(&irq, 0x200).await;
        let starts = Rc::new(Cell::new(0));
        let order = Rc::new(RefCell::new(Vec::new()));
        let state = runner.cpu_state();
        runner
            .install_hooks([real_mode_interrupt::hook(0x200, {
                let (cpu, starts, order) = (state.clone(), starts.clone(), order.clone());
                async move || {
                    let index = starts.get();
                    starts.set(index + 1);
                    let _drop = DropOrder {
                        index,
                        order: order.clone(),
                    };
                    real_mode_interrupt::with_interrupts_enabled(
                        &cpu,
                        std::future::pending::<()>(),
                    )
                    .await;
                    unreachable!()
                }
            })])
            .await
            .unwrap();
        pending(runner.run(10));
        for _ in 0..2 {
            irq.borrow_mut().raise();
            pending(runner.run(10));
        }
        assert_eq!(starts.get(), 3);
        let before = state.snapshot();
        if stopping {
            runner.stop_handle().stop();
            assert_eq!(runner.run(0).await, ExecutionExit::BudgetExhausted);
            assert!(order.borrow().is_empty());
            assert_eq!(runner.run(10).await, ExecutionExit::Host(HostExit::Return));
            assert_eq!(*order.borrow(), [2, 1, 0]);
        }
        drop(runner);
        assert_eq!(*order.borrow(), [2, 1, 0]);
        assert_eq!(state.snapshot(), before);
        assert!(state.interrupts_enabled());
    }
}

async fn replacement_irq_waker() {
    use std::{
        future::Future,
        pin::pin,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        task::{Context, Poll, Wake, Waker},
    };
    #[derive(Default)]
    struct Wakes(AtomicUsize);
    impl Wake for Wakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    runner
        .install_hooks([real_mode_interrupt::hook(0x200, {
            let cpu = runner.cpu_state();
            async move || {
                cpu.set_interrupts_enabled(true);
                std::future::pending::<()>().await;
                unreachable!()
            }
        })])
        .await
        .unwrap();
    let previous = Arc::new(Wakes::default());
    let current = Arc::new(Wakes::default());
    let old_waker = Waker::from(previous.clone());
    let new_waker = Waker::from(current.clone());
    {
        let mut run = pin!(runner.run(10));
        assert!(run
            .as_mut()
            .poll(&mut Context::from_waker(&old_waker))
            .is_pending());
    }
    {
        let mut run = pin!(runner.run(1));
        let mut cx = Context::from_waker(&new_waker);
        assert!(run.as_mut().poll(&mut cx).is_pending());
        irq.borrow_mut().raise();
        assert_eq!(previous.0.load(Ordering::Relaxed), 0);
        assert_eq!(current.0.load(Ordering::Relaxed), 1);
        assert_eq!(
            run.as_mut().poll(&mut cx),
            Poll::Ready(ExecutionExit::BudgetExhausted)
        );
    }
    assert_eq!(runner.cpu_state().eip(), 0x306);
    assert_eq!(irq.borrow().acknowledged, 1);
}

async fn abandon_outer_while_inner_waits() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x200).await;
    let inputs = [0; 2].map(|_| Rc::new(Input::default()));
    let starts = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    let state = runner.cpu_state();
    runner
        .install_hooks([real_mode_interrupt::hook(0x200, {
            let (cpu, sources, starts, drops) =
                (state.clone(), inputs.clone(), starts.clone(), drops.clone());
            let stop = runner.stop_handle();
            async move || {
                let index = starts.get();
                starts.set(index + 1);
                let _operation = Operation(drops.clone());
                let value =
                    real_mode_interrupt::with_interrupts_enabled(&cpu, sources[index].read()).await;
                assert_eq!(index, 1, "the abandoned outer future cannot finish");
                cpu.set_ax(value);
                stop.stop();
            }
        })])
        .await
        .unwrap();
    pending(runner.run(10));
    irq.borrow_mut().raise();
    pending(runner.run(10));
    assert_eq!(starts.get(), 2);
    let before = state.snapshot();
    assert!(state.interrupts_enabled());
    runner.abandon_suspended_calls();
    assert_eq!(state.snapshot(), before);
    assert_eq!(drops.get(), 1);
    inputs[0].send(10);
    pending(runner.run(10));
    assert_eq!(inputs[0].consumed.get(), 0);
    assert_eq!(drops.get(), 1);
    inputs[1].send(20);
    assert_eq!(runner.run(10).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(state.ax(), 20);
    assert_eq!(inputs[1].consumed.get(), 1);
    assert_eq!(drops.get(), 2);
}

test_backends!(
    original_hook_and_stack_start_a_new_invocation_after_redirection,
    redirected_hook
);
test_backends!(
    explicit_abandonment_releases_work_without_changing_live_cpu,
    explicit_abandonment
);
test_backends!(
    stop_and_destruction_release_innermost_futures_first_without_restoring_if,
    destruction_order
);
test_backends!(
    device_irqs_wake_the_replacement_run_waiter,
    replacement_irq_waker
);
test_backends!(
    abandoning_parked_calls_preserves_the_active_inner_future,
    abandon_outer_while_inner_waits
);
