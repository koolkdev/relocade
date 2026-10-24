mod support;
use relocade_machine::{real_mode_interrupt, Hook, HookResult};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(Default)]
struct Input {
    values: VecDeque<u16>,
    waker: Option<Waker>,
}

#[derive(Default)]
struct State {
    input: RefCell<Input>,
    started: Cell<u32>,
    first_awaits: Cell<u32>,
    completed: Cell<u32>,
    dropped: Cell<u32>,
}

impl State {
    async fn read(&self) -> u16 {
        poll_fn(|cx| {
            let mut input = self.input.borrow_mut();
            match input.values.pop_front() {
                Some(value) => Poll::Ready(value),
                None => {
                    input.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
        .await
    }

    fn send(&self, values: &[u16]) {
        let waker = {
            let mut input = self.input.borrow_mut();
            input.values.extend(values);
            input.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

struct Operation(Rc<State>);

impl Drop for Operation {
    fn drop(&mut self) {
        self.0.dropped.set(self.0.dropped.get() + 1);
    }
}

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn assert_pending(future: impl Future) {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut cx).is_pending());
}

async fn check_pending_calls() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let state = Rc::new(State::default());
    let mut runtime = support::create_cpu(state.clone(), config).await.unwrap();
    runtime.context().cpu().set_sp(0xff00);
    runtime.context().cpu().set_eflags(0x202);
    runtime
        .context()
        .memory()
        .write(0x100, &[0xcd, 0x21, 0x40, 0xeb, 0xfb]) // INT 21h; INC AX; JMP 0100
        .unwrap();
    runtime
        .context()
        .memory()
        .write(0x84, &[0, 2, 0, 0])
        .unwrap();
    runtime
        .context()
        .memory()
        .write(0x200, &real_mode_interrupt::HOOK_STUB)
        .unwrap();
    runtime
        .install_hooks([Hook::<Rc<State>>::new(0x200, |mut call| async move {
            let state = call.with_state(Clone::clone);
            state.started.set(state.started.get() + 1);
            let operation = Operation(state.clone());
            let sum = {
                // A view can cross awaits without retaining a backend borrow.
                let mut cpu = call.cpu();
                let first = operation.0.read().await;
                cpu.set_bx(first);
                operation
                    .0
                    .first_awaits
                    .set(operation.0.first_awaits.get() + 1);
                // This reference to a local remains valid while the future is pinned.
                let values = [first];
                let saved = &values[0];
                let second = operation.0.read().await;
                assert_eq!(cpu.bx(), *saved);
                let sum = *saved + second;
                cpu.set_bx(sum);
                sum
            };
            drop(operation);
            state.completed.set(state.completed.get() + 1);
            call.memory().write(0x300, &sum.to_le_bytes()).unwrap();
            real_mode_interrupt::return_from_interrupt(&mut call)
        })])
        .await
        .unwrap();

    assert_pending(runtime.run(6));
    let shared = runtime.memory();
    let reader = shared.view();
    let suspended = runtime.context().cpu().snapshot();
    assert_eq!(suspended.instruction_count, 1);
    assert_eq!(suspended.registers.esp, 0xfefa);
    assert_pending(runtime.run(100));
    assert_eq!(runtime.context().cpu().snapshot(), suspended);
    assert_eq!(runtime.run(0).await, ExecutionExit::BudgetExhausted);
    // An unpolled run has no effect on the retained service.
    drop(runtime.run(5));
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    {
        let mut run = pin!(runtime.run(5));
        assert!(run.as_mut().poll(&mut cx).is_pending());
        state.send(&[10]);
        assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
        assert!(run.as_mut().poll(&mut cx).is_pending());
        assert_eq!(state.first_awaits.get(), 1);
        assert_eq!(state.started.get(), 1);
        assert_eq!(state.dropped.get(), 0);
        state.send(&[20]);
        assert!(run.as_mut().poll(&mut cx).is_pending());
        // Independent readers see writes from the completed operation while
        // the next service waits.
        assert_eq!(reader.read_u16(0x300).unwrap(), 30);
        assert_eq!(state.started.get(), 2);
        assert_eq!(state.completed.get(), 1);
        state.send(&[30, 40]);
        assert!(matches!(
            run.as_mut().poll(&mut cx),
            Poll::Ready(ExecutionExit::BudgetExhausted)
        ));
    }
    // Both suspensions shared the original target, ending at the second IRET.
    assert_eq!(state.started.get(), 2);
    assert_eq!(state.first_awaits.get(), 2);
    assert_eq!(state.completed.get(), 2);
    assert_eq!(state.dropped.get(), 2);
    let cpu = runtime.context().cpu().snapshot();
    assert_eq!(cpu.instruction_count, 6);
    assert_eq!(cpu.eip, 0x102);
    assert_eq!(cpu.registers.eax, 1);
    assert_eq!(cpu.registers.ebx, 70);
    assert_eq!(cpu.registers.esp, 0xff00);
    assert_eq!(cpu.flags.eflags(), 0x202);
    let mut bytes = [0; 2];
    runtime.context().memory().read(0x300, &mut bytes).unwrap();
    assert_eq!(bytes, [70, 0]);

    assert_pending(runtime.run(10));
    runtime.cancel_pending_call();
    assert_eq!(state.dropped.get(), 3);
    assert_eq!(state.completed.get(), 2);

    // Dropping a waiter leaves the future, its local references and partial
    // effects intact. Embedding access does not contend with a backend borrow.
    runtime.context().cpu().set_eip(0x100);
    runtime.context().cpu().set_sp(0xff00);
    {
        let mut run = pin!(runtime.run(10));
        assert!(run.as_mut().poll(&mut cx).is_pending());
        state.send(&[55]);
        assert!(run.as_mut().poll(&mut cx).is_pending());
    }
    assert_eq!(runtime.context().cpu().bx(), 55);
    assert_eq!(state.dropped.get(), 3);
    assert_eq!(state.started.get(), 4);
    assert_eq!(state.completed.get(), 2);
    let stopped = runtime.context();
    stopped.with_state_mut(|state| {
        assert_eq!(state.started.get(), 4);
        stopped.cpu().set_cx(123);
    });
    assert_eq!(stopped.cpu().cx(), 123);
    state.send(&[65]);
    assert_eq!(runtime.run(0).await, ExecutionExit::BudgetExhausted);
    assert_eq!(state.completed.get(), 2);
    assert_eq!(runtime.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu().bx(), 120);
    assert_eq!(state.started.get(), 4);
    assert_eq!(state.completed.get(), 3);
    assert_eq!(state.dropped.get(), 4);

    // The embedding can replace the continuation and clear the interrupted call.
    runtime.context().cpu().set_eip(0x400);
    runtime.cancel_pending_call();
    runtime
        .context()
        .memory()
        .write(0x400, &[0xeb, 0xfe])
        .unwrap();
    assert_eq!(runtime.run(1).await, ExecutionExit::BudgetExhausted);
    let count = runtime.context().cpu().instruction_count();
    runtime
        .context()
        .memory()
        .write(0x500, &[0x0f, 0x0b]) // UD2: completing must not enter the guest.
        .unwrap();
    runtime
        .install_hooks([Hook::new(0x400, |call| async move {
            // Completion returns directly even if the continuation names guest code.
            call.cpu().set_eip(0x500);
            let value = std::future::ready(99).await;
            call.cpu().set_bx(value);
            let value = std::future::ready(value + 1).await;
            assert_eq!(call.cpu().bx(), 99);
            call.cpu().set_cx(value);
            HookResult::Exit(ExecutionExit::HostReturn)
        })])
        .await
        .unwrap();
    assert_eq!(runtime.run(1).await, ExecutionExit::HostReturn);
    assert_eq!(runtime.context().cpu().cx(), 100);
    assert_eq!(runtime.context().cpu().eip(), 0x500);
    assert_eq!(runtime.context().cpu().instruction_count(), count);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pending_services_survive_waiter_changes_and_preserve_each_run_budget() {
    pollster::block_on(check_pending_calls());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pending_services_survive_waiter_changes_and_preserve_each_run_budget() {
    check_pending_calls().await;
}

async fn check_pending_replacement() {
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
    let state = Rc::new(State::default());
    let mut runtime = support::create_cpu(state.clone(), config).await.unwrap();
    let registration = Rc::new(());
    let released = Rc::downgrade(&registration);
    let saved = Rc::new(RefCell::new(None));
    let capture = saved.clone();
    runtime
        .install_hooks([Hook::<Rc<State>>::new(0x400, move |call| {
            let registration = registration.clone();
            let saved = capture.clone();
            async move {
                *saved.borrow_mut() = Some(call.clone());
                let input = call.with_state(Clone::clone);
                let value = input.read().await;
                call.cpu().set_ax(value);
                drop(registration);
                HookResult::Exit(ExecutionExit::HostReturn)
            }
        })])
        .await
        .unwrap();
    assert_pending(runtime.run(1));
    runtime
        .install_hooks([Hook::new(0x400, |call| async move {
            call.cpu().set_ax(99);
            HookResult::Exit(ExecutionExit::HostReturn)
        })])
        .await
        .unwrap();
    assert!(released.upgrade().is_some());
    state.send(&[42]);
    assert_eq!(runtime.run(1).await, ExecutionExit::HostReturn);
    assert_eq!(runtime.context().cpu().ax(), 42);
    assert!(released.upgrade().is_none());
    #[cfg(not(target_arch = "wasm32"))]
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        saved.borrow().as_ref().unwrap().cpu().ax();
    }))
    .is_err());
    assert_eq!(runtime.run(1).await, ExecutionExit::HostReturn);
    assert_eq!(runtime.context().cpu().ax(), 99);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn replacement_keeps_the_pending_invocation_and_releases_the_old_registration() {
    pollster::block_on(check_pending_replacement());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn replacement_keeps_the_pending_invocation_and_releases_the_old_registration() {
    check_pending_replacement().await;
}
