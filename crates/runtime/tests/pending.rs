use relocade_machine::{MachineConfig, ReadMemory};
use relocade_runtime::{real_mode_interrupt, Error, Hook, Runtime, RuntimeState};
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

use relocade_wasm_runtime::Backend;

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
    let config = MachineConfig::new(
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
    let backend = Backend::with_state(RuntimeState::new(state.clone())).unwrap();
    let mut runtime = Runtime::new(backend, config).await.unwrap();
    runtime.machine().cpu().set_sp(0xff00);
    runtime.machine().cpu().set_eflags(0x202);
    runtime
        .machine()
        .memory()
        .write(0x100, &[0xcd, 0x21, 0x40, 0xeb, 0xfb]) // INT 21h; INC AX; JMP 0100
        .unwrap();
    runtime
        .machine()
        .memory()
        .write(0x84, &[0, 2, 0, 0])
        .unwrap();
    runtime
        .machine()
        .memory()
        .write(0x200, &real_mode_interrupt::HOOK_STUB)
        .unwrap();
    runtime
        .install_hooks([Hook::<Rc<State>>::new(0x200, |call| {
            call.state().started.set(call.state().started.get() + 1);
            let operation = Operation(call.state().clone());
            call.defer(async move |call| {
                let sum = {
                    // The CPU view itself stays borrowed through both awaits.
                    let mut cpu = call.cpu();
                    let first = operation.0.read().await;
                    cpu.set_bx(first);
                    operation
                        .0
                        .first_awaits
                        .set(operation.0.first_awaits.get() + 1);
                    let second = operation.0.read().await;
                    let sum = cpu.bx() + second;
                    cpu.set_bx(sum);
                    sum
                };
                drop(operation);
                call.state().completed.set(call.state().completed.get() + 1);
                call.memory().write(0x300, &sum.to_le_bytes()).unwrap();
                real_mode_interrupt::return_from_interrupt(call)
            })
        })])
        .await
        .unwrap();

    assert_eq!(runtime.run(6).unwrap(), ExecutionExit::Deferred);
    let reader = runtime.shared_memory();
    let suspended = runtime.machine().cpu().snapshot();
    assert_eq!(suspended.instruction_count, 1);
    assert_eq!(suspended.registers.esp, 0xfefa);
    assert_eq!(runtime.run(100).unwrap(), ExecutionExit::Deferred);
    assert_eq!(runtime.machine().cpu().snapshot(), suspended);
    assert_eq!(
        runtime.run_async(0).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    // An unpolled run has not started or cancelled the queued handler.
    drop(runtime.run_async(5));
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    {
        let mut run = pin!(runtime.run_async(5));
        assert!(run.as_mut().poll(&mut cx).is_pending());
        state.send(&[10]);
        assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
        assert!(run.as_mut().poll(&mut cx).is_pending());
        assert_eq!(state.first_awaits.get(), 1);
        assert_eq!(state.started.get(), 1);
        assert_eq!(state.dropped.get(), 0);
        state.send(&[20]);
        assert!(run.as_mut().poll(&mut cx).is_pending());
        // The next deferred operation holds the backend context. Display/device
        // readers still see writes from the completed host operation.
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
    let cpu = runtime.machine().cpu().snapshot();
    assert_eq!(cpu.instruction_count, 6);
    assert_eq!(cpu.eip, 0x102);
    assert_eq!(cpu.registers.eax, 1);
    assert_eq!(cpu.registers.ebx, 70);
    assert_eq!(cpu.registers.esp, 0xff00);
    assert_eq!(cpu.flags.eflags(), 0x202);
    let mut bytes = [0; 2];
    runtime.machine().memory().read(0x300, &mut bytes).unwrap();
    assert_eq!(bytes, [70, 0]);

    assert_eq!(runtime.run(10).unwrap(), ExecutionExit::Deferred);
    runtime.cancel_pending_call();
    assert_eq!(state.dropped.get(), 3);
    assert_eq!(state.completed.get(), 2);

    // Dropping an active handler releases its borrows and rejects accidental
    // replay. Effects performed before cancellation remain visible.
    runtime.machine().cpu().set_eip(0x100);
    runtime.machine().cpu().set_sp(0xff00);
    {
        let mut run = pin!(runtime.run_async(10));
        assert!(run.as_mut().poll(&mut cx).is_pending());
        state.send(&[55]);
        assert!(run.as_mut().poll(&mut cx).is_pending());
    }
    assert_eq!(runtime.machine().cpu().bx(), 55);
    assert_eq!(state.dropped.get(), 4);
    assert!(matches!(runtime.run(1), Err(Error::CancelledHostCall)));
    assert!(matches!(
        runtime.run_async(1).await,
        Err(Error::CancelledHostCall)
    ));
    assert_eq!(state.started.get(), 4);
    assert_eq!(state.completed.get(), 2);

    // The embedding can replace the continuation and clear the interrupted call.
    runtime.machine().cpu().set_eip(0x400);
    runtime.cancel_pending_call();
    runtime
        .machine()
        .memory()
        .write(0x400, &[0xeb, 0xfe])
        .unwrap();
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    let count = runtime.machine().cpu().instruction_count();
    runtime
        .machine()
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
                std::ops::ControlFlow::Break(ExecutionExit::HostReturn)
            })
        })])
        .await
        .unwrap();
    assert_eq!(
        runtime.run_async(1).await.unwrap(),
        ExecutionExit::HostReturn
    );
    assert_eq!(runtime.machine().cpu().cx(), 100);
    assert_eq!(runtime.machine().cpu().eip(), 0x500);
    assert_eq!(runtime.machine().cpu().instruction_count(), count);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn deferred_handlers_borrow_live_state_and_preserve_the_budget() {
    pollster::block_on(check_pending_calls());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn deferred_handlers_borrow_live_state_and_preserve_the_budget() {
    check_pending_calls().await;
}
