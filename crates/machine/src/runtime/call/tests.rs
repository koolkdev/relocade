use super::*;
use relocade_wasm_runtime::{Backend, Memory, WasmBackend, WasmMemoryAccess};
use relocade_x86::{abi, Compiler, ExecutionProfile};
use std::{
    future::{pending, poll_fn, ready, Future},
    pin::Pin,
    task::Waker,
};

struct Registers {
    backend: Backend,
    memory: Memory,
}

impl Registers {
    fn new() -> Self {
        let mut backend = Backend::new().unwrap();
        let memory = backend.create_memory(abi::EXECUTION_CONTEXT).unwrap();
        Self { backend, memory }
    }

    fn storage(&mut self) -> CpuStorage<'_> {
        self.backend.memory(&self.memory).into_prefix()
    }

    fn cpu(&mut self) -> CpuView<CpuStorage<'_>> {
        CpuView::new(target(), self.storage())
    }
}

fn target() -> ExecutionTarget {
    Compiler::new(ExecutionProfile::Real16).target()
}

fn cx() -> Context<'static> {
    Context::from_waker(Waker::noop())
}

fn suspended(poll: CallPoll) -> Call {
    match poll {
        CallPoll::Pending(call) => call,
        CallPoll::Ready(_) => panic!("expected a pending service"),
    }
}

fn completed(poll: CallPoll) {
    assert!(matches!(poll, CallPoll::Ready(HookResult::Continue)));
}

#[test]
fn ready_awaits_and_batches_share_one_binding() {
    let scope = Rc::default();
    let handler: Rc<dyn Handler> = Rc::new(async |mut cpu: HookCpu<'_>| {
        cpu.set_ax(ready(12).await);
        cpu.with(|cpu| cpu.set_bx(cpu.ax() + 1));
        HookResult::Continue
    });
    let mut registers = Registers::new();
    completed(Call::start(
        &handler,
        &scope,
        target(),
        registers.storage(),
        &mut cx(),
    ));
    assert_eq!((registers.cpu().ax(), registers.cpu().bx()), (12, 13));
    assert!(scope.storage.get().is_none());
    assert_eq!(Rc::strong_count(&handler), 1);
    assert_eq!(Rc::strong_count(&scope), 1);
}

#[test]
fn nested_invocations_keep_locals_and_capture_borrows_until_selected() {
    let scope = Rc::default();
    let started = Rc::new(Cell::new(0));
    let count = started.clone();
    let text = String::from("retained capture");
    let handler: Rc<dyn Handler> = Rc::new(async move |mut cpu: HookCpu<'_>| {
        count.set(count.get() + 1);
        let local = [count.get()];
        let saved = &local[0];
        let capture = text.as_str();
        let mut first = true;
        poll_fn(|_| {
            if std::mem::replace(&mut first, false) {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;
        assert_eq!(capture, "retained capture");
        cpu.set_ax(*saved);
        HookResult::Continue
    });
    let mut registers = Registers::new();
    let outer = suspended(Call::start(
        &handler,
        &scope,
        target(),
        registers.storage(),
        &mut cx(),
    ));
    let inner = suspended(Call::start(
        &handler,
        &scope,
        target(),
        registers.storage(),
        &mut cx(),
    ));
    // Handler and slot addresses remain stable when their owner moves or releases
    // its references. Each retained invocation owns those dependencies itself.
    let weak = Rc::downgrade(&handler);
    drop(handler);
    drop(scope);
    let mut frames = vec![outer, inner];
    completed(frames.pop().unwrap().poll(registers.storage(), &mut cx()));
    assert_eq!(registers.cpu().ax(), 2);
    assert!(weak.upgrade().is_some());
    completed(frames.pop().unwrap().poll(registers.storage(), &mut cx()));
    assert_eq!(registers.cpu().ax(), 1);
    assert!(weak.upgrade().is_none());
}

struct Cleanup<'a> {
    cpu: HookCpu<'a>,
    drops: Rc<Cell<u32>>,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        if self.cpu.is_bound() {
            self.cpu.set_eflags(0x202);
        }
    }
}

#[test]
fn abandoning_an_outer_call_cannot_restore_its_flags_after_an_inner_call() {
    let scope = Rc::default();
    let drops = Rc::new(Cell::new(0));
    let events = drops.clone();
    let outer: Rc<dyn Handler> = Rc::new(async move |cpu: HookCpu<'_>| {
        let _cleanup = Cleanup {
            cpu,
            drops: events.clone(),
        };
        pending::<HookResult>().await
    });
    let inner: Rc<dyn Handler> = Rc::new(async |mut cpu: HookCpu<'_>| {
        cpu.set_eflags(2);
        HookResult::Continue
    });
    let mut registers = Registers::new();
    let parked = suspended(Call::start(
        &outer,
        &scope,
        target(),
        registers.storage(),
        &mut cx(),
    ));
    completed(Call::start(
        &inner,
        &scope,
        target(),
        registers.storage(),
        &mut cx(),
    ));
    drop(parked);
    assert_eq!(drops.get(), 1);
    assert_eq!(registers.cpu().eflags(), 2);
}

struct CompleteAfterPending<'a> {
    cpu: HookCpu<'a>,
    first: bool,
}
impl Future for CompleteAfterPending<'_> {
    type Output = HookResult;
    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<HookResult> {
        if std::mem::replace(&mut self.first, false) {
            Poll::Pending
        } else {
            Poll::Ready(HookResult::Continue)
        }
    }
}
impl Drop for CompleteAfterPending<'_> {
    fn drop(&mut self) {
        self.cpu.set_ax(55);
    }
}
fn custom_future(mut cpu: HookCpu<'_>) -> CompleteAfterPending<'_> {
    let first = cpu.bx() == 0;
    cpu.set_bx(99); // Construction, before any Future::poll, also has CPU access.
    CompleteAfterPending { cpu, first }
}

#[test]
fn custom_future_construction_and_ready_destruction_are_bound() {
    for pending_first in [false, true] {
        let scope = Rc::default();
        let handler: Rc<dyn Handler> = Rc::new(custom_future);
        let mut registers = Registers::new();
        registers.cpu().set_bx(u16::from(!pending_first));
        let polled = Call::start(&handler, &scope, target(), registers.storage(), &mut cx());
        assert_eq!(registers.cpu().bx(), 99);
        if pending_first {
            completed(suspended(polled).poll(registers.storage(), &mut cx()));
        } else {
            completed(polled);
        }
        assert_eq!(registers.cpu().ax(), 55);
        assert!(scope.storage.get().is_none());
    }
}

#[test]
fn cancellation_rejects_unconditional_cpu_access() {
    let scope = Rc::default();
    let handler: Rc<dyn Handler> = Rc::new(custom_future);
    let mut registers = Registers::new();
    registers.cpu().set_ax(4);
    let call = suspended(Call::start(
        &handler,
        &scope,
        target(),
        registers.storage(),
        &mut cx(),
    ));
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(call))).is_err());
    assert_eq!(registers.cpu().ax(), 4);
    assert!(scope.storage.get().is_none());
}

#[test]
fn unwinding_first_and_later_polls_drops_locals_before_clearing_access() {
    for pending_first in [false, true] {
        let scope = Rc::default();
        let drops = Rc::new(Cell::new(0));
        let events = drops.clone();
        let handler: Rc<dyn Handler> = Rc::new(async move |cpu: HookCpu<'_>| {
            let _cleanup = Cleanup {
                cpu,
                drops: events.clone(),
            };
            let mut first = pending_first;
            poll_fn(|_| {
                if std::mem::replace(&mut first, false) {
                    Poll::Pending
                } else {
                    panic!("poll")
                }
            })
            .await
        });
        let mut registers = Registers::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let poll = Call::start(&handler, &scope, target(), registers.storage(), &mut cx());
            if let CallPoll::Pending(call) = poll {
                call.poll(registers.storage(), &mut cx());
            }
        }));
        assert!(result.is_err());
        assert_eq!(drops.get(), 1);
        assert_eq!(registers.cpu().eflags(), 0x202);
        assert!(scope.storage.get().is_none());
        // Reusing the same slot must succeed after either unwind path.
        let good: Rc<dyn Handler> = Rc::new(async |_: HookCpu<'_>| HookResult::Continue);
        completed(Call::start(
            &good,
            &scope,
            target(),
            registers.storage(),
            &mut cx(),
        ));
    }
}
