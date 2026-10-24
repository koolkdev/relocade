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
        .install_hooks([Hook::<Rc<State>>::asynchronous(0x200, |call| async move {
            call.state().started.set(call.state().started.get() + 1);
            let operation = Operation(call.state().clone());
            let sum = {
                // This handle borrows the call identity, never live CPU state.
                let cpu = call.cpu();
                let first = operation.0.read().await;
                cpu.set_bx(first);
                operation
                    .0
                    .first_awaits
                    .set(operation.0.first_awaits.get() + 1);
                let second = operation.0.read().await;
                assert_eq!(cpu.bx(), first);
                let sum = first + second;
                cpu.set_bx(sum);
                sum
            };
            drop(operation);
            call.state().completed.set(call.state().completed.get() + 1);
            call.memory().write(0x300, &sum.to_le_bytes()).unwrap();
            call.return_from_interrupt()
        })])
        .await
        .unwrap();

    assert!(support::poll_once(runtime.run(6)).is_pending());
    let shared = runtime.memory();
    let reader = shared.view();
    let suspended = runtime.context().cpu().snapshot();
    assert_eq!(suspended.instruction_count, 1);
    assert_eq!(suspended.registers.esp, 0xfefa);
    assert!(support::poll_once(runtime.run(100)).is_pending());
    assert_eq!(runtime.context().cpu().snapshot(), suspended);
    assert_eq!(
        runtime.run(0).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    // Creating and dropping an unpolled waiter leaves the active handler unchanged.
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
        // Shared memory readers see completed writes while the next service waits.
        assert_eq!(reader.read_u16(0x300).unwrap(), 30);
        assert_eq!(state.started.get(), 2);
        assert_eq!(state.completed.get(), 1);
        state.send(&[30, 40]);
        assert!(matches!(
            run.as_mut().poll(&mut cx),
            Poll::Ready(Ok(ExecutionExit::BudgetExhausted))
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

    assert!(support::poll_once(runtime.run(10)).is_pending());
    runtime.cancel_pending_call();
    assert_eq!(state.dropped.get(), 3);
    assert_eq!(state.completed.get(), 2);

    // Dropping a waiter keeps the same operation and its locals alive.
    // Effects performed before explicit cancellation remain visible.
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
    assert!(support::poll_once(runtime.run(1)).is_pending());
    state.send(&[65]);
    assert_eq!(
        runtime.run(1).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(runtime.context().cpu().bx(), 120);
    assert_eq!(state.started.get(), 4);
    assert_eq!(state.completed.get(), 3);
    assert_eq!(state.dropped.get(), 4);

    // Explicit cancellation drops a partially completed operation once.
    runtime.context().cpu().set_eip(0x100);
    runtime.context().cpu().set_sp(0xff00);
    {
        let mut run = pin!(runtime.run(10));
        assert!(run.as_mut().poll(&mut cx).is_pending());
        state.send(&[77]);
        assert!(run.as_mut().poll(&mut cx).is_pending());
    }
    runtime.cancel_pending_call();
    assert_eq!(runtime.context().cpu().bx(), 77);
    assert_eq!(state.started.get(), 5);
    assert_eq!(state.completed.get(), 3);
    assert_eq!(state.dropped.get(), 5);

    // The embedding can replace the continuation and clear the interrupted call.
    runtime.context().cpu().set_eip(0x400);
    runtime.cancel_pending_call();
    runtime
        .context()
        .memory()
        .write(0x400, &[0xeb, 0xfe])
        .unwrap();
    assert_eq!(
        support::ready(runtime.run(1)).unwrap(),
        ExecutionExit::BudgetExhausted
    );
    let count = runtime.context().cpu().instruction_count();
    runtime
        .context()
        .memory()
        .write(0x500, &[0x0f, 0x0b]) // UD2: completing must not enter the guest.
        .unwrap();
    runtime
        .install_hooks([Hook::new(0x400, |call| {
            // The continuation need not name this hook. Awaiting and completing
            // the service must not enter guest code at this address.
            call.cpu().set_eip(0x500);
            call.defer(async |call| {
                let value = std::future::ready(99).await;
                call.cpu().set_bx(value);
                let value = std::future::ready(value + 1).await;
                assert_eq!(call.cpu().bx(), 99);
                call.cpu().set_cx(value);
                HookResult::Exit(ExecutionExit::HostReturn)
            })
        })])
        .await
        .unwrap();
    assert_eq!(runtime.run(1).await.unwrap(), ExecutionExit::HostReturn);
    assert_eq!(runtime.context().cpu().cx(), 100);
    assert_eq!(runtime.context().cpu().eip(), 0x500);
    assert_eq!(runtime.context().cpu().instruction_count(), count);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn owned_handlers_preserve_locals_and_budget_across_waiters() {
    pollster::block_on(check_pending_calls());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn owned_handlers_preserve_locals_and_budget_across_waiters() {
    check_pending_calls().await;
}

async fn check_ready_calls_and_handle_lifetime() {
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
    let mut runtime = support::create_cpu((), config).await.unwrap();
    let calls = Rc::new(Cell::new(0));
    let retained = Rc::new(RefCell::new(None));
    let factory_calls = calls.clone();
    let factory_retained = retained.clone();
    runtime
        .install_hooks([Hook::asynchronous(0x100, move |call| {
            let calls = factory_calls.clone();
            let retained = factory_retained.clone();
            async move {
                calls.set(calls.get() + 1);
                *retained.borrow_mut() = Some(call.clone());
                let value = std::future::ready(42).await;
                call.cpu().set_ax(value);
                HookResult::Exit(ExecutionExit::HostReturn)
            }
        })])
        .await
        .unwrap();
    assert_eq!(
        support::ready(runtime.run(0)).unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(calls.get(), 0);
    for expected in 1..=4 {
        let mut cx = Context::from_waker(Waker::noop());
        {
            let mut run = pin!(runtime.run(10));
            assert!(matches!(
                run.as_mut().poll(&mut cx),
                Poll::Ready(Ok(ExecutionExit::HostReturn))
            ));
        }
        assert_eq!(calls.get(), expected);
        assert_eq!(runtime.context().cpu().ax(), 42);
    }
    assert_eq!(
        support::ready(runtime.run(10)).unwrap(),
        ExecutionExit::HostReturn
    );
    assert_eq!(calls.get(), 5);
    let stale = retained.borrow_mut().take().unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| stale.cpu().ax())).is_err());
    drop(runtime);
    #[cfg(not(target_arch = "wasm32"))]
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| stale.cpu().ax())).is_err());
    drop(stale);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ready_calls_finish_inline_and_have_scoped_identity() {
    pollster::block_on(check_ready_calls_and_handle_lifetime());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn ready_calls_finish_inline_and_have_scoped_identity() {
    check_ready_calls_and_handle_lifetime().await;
}
