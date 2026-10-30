mod support;

use relocade_machine::{real_mode_interrupt, Cpu, Device, Hook, HookResult, IoBus, Memory, Runner};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap, Segments,
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

#[derive(Default)]
struct Input {
    values: RefCell<VecDeque<u16>>,
    waker: RefCell<Option<Waker>>,
    consumed: Cell<usize>,
}

impl Input {
    async fn read(&self) -> u16 {
        poll_fn(|cx| {
            if let Some(value) = self.values.borrow_mut().pop_front() {
                self.consumed.set(self.consumed.get() + 1);
                Poll::Ready(value)
            } else {
                *self.waker.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }

    fn send(&self, value: u16) {
        self.values.borrow_mut().push_back(value);
        if let Some(waker) = self.waker.borrow_mut().take() {
            waker.wake();
        }
    }
}

#[derive(Default)]
struct Irq {
    pending: bool,
    acknowledged: usize,
    waker: Option<Waker>,
}

impl Irq {
    fn raise(&mut self) {
        self.pending = true;
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

impl Device for Irq {
    fn poll_interrupt(&mut self, cx: &mut Context<'_>) -> Poll<u8> {
        if self.pending {
            self.pending = false;
            self.acknowledged += 1;
            Poll::Ready(0x60)
        } else {
            self.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

struct Operation(Rc<Cell<usize>>);
impl Drop for Operation {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

async fn cpu(irq: &Rc<RefCell<Irq>>, handler: u16) -> Cpu {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..Default::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let runner = Runner::with_clock(support::clock::ManualClock::default());
    let io = IoBus::new([runner.connect(irq.clone(), [], [])]);
    let memory = Memory::new(config.machine).unwrap();
    let mut cpu = Cpu::new(memory, config.cpu, io, ()).await.unwrap();
    cpu.with(|context| {
        context.cpu.set_sp(0x8000);
        context.cpu.set_eflags(0x243);
        context
            .memory
            .write(0x100, &[0xcd, 0x21, 0xeb, 0xfe])
            .unwrap();
        context.memory.write(0x84, &[0, 2, 0, 0]).unwrap();
        context
            .memory
            .write(0x180, &[handler as u8, (handler >> 8) as u8, 0, 0])
            .unwrap();
        context
            .memory
            .write(0x200, &real_mode_interrupt::HOOK_STUB)
            .unwrap();
        // INC word [0500]; JMP 0306; IRET. The jump creates an ISR budget boundary.
        context
            .memory
            .write(0x300, &[0xff, 0x06, 0, 5, 0xeb, 0, 0xcf])
            .unwrap();
    });
    cpu
}

fn pending(future: impl Future) {
    assert!(pin!(future)
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
}

async fn check_retained_future() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut cpu = cpu(&irq, 0x300).await;
    let input = Rc::new(Input::default());
    input.send(10);
    let started = Rc::new(Cell::new(0));
    let completed = Rc::new(Cell::new(0));
    let dropped = Rc::new(Cell::new(0));
    let (source, starts, finishes, drops) = (
        input.clone(),
        started.clone(),
        completed.clone(),
        dropped.clone(),
    );
    cpu.install_hooks([Hook::new(0x200, 0x201, async move |call| {
        starts.set(starts.get() + 1);
        let _operation = Operation(drops.clone());
        call.with(|context| context.cpu.set_interrupts_enabled(true));
        let first = source.read().await;
        let local = [first];
        let saved = &local[0];
        // No wrapper grants permission: live IF controls this ordinary await.
        let second = source.read().await;
        let sum = *saved + second;
        finishes.set(finishes.get() + 1);
        call.with(|context| {
            assert_eq!(context.cpu.eflags(), 0x243);
            context.cpu.set_ax(sum);
            real_mode_interrupt::return_from_interrupt(&mut context.cpu)
        })
    })])
    .await
    .unwrap();

    // IRQ completion precedes input readiness while the same run is pending.
    {
        let mut run = pin!(cpu.run(20));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(run.as_mut().poll(&mut cx).is_pending());
        irq.borrow_mut().raise();
        assert!(run.as_mut().poll(&mut cx).is_pending());
    }
    assert_eq!(cpu.memory().view().read_u16(0x500).unwrap(), 1);
    assert_eq!(started.get(), 1);
    assert_eq!(input.consumed.get(), 1);
    assert_eq!(completed.get(), 0);
    assert_eq!(dropped.get(), 0);

    // A new waiter gives the ISR one quantum while preserving the parked future.
    irq.borrow_mut().raise();
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(cpu.with(|context| context.cpu.eip()), 0x306);
    assert_eq!(cpu.memory().view().read_u16(0x500).unwrap(), 2);
    input.send(20);
    assert_eq!(cpu.run(0).await, ExecutionExit::BudgetExhausted);
    assert_eq!(input.consumed.get(), 1);
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted); // IRQ IRET only.
    assert_eq!(cpu.with(|context| context.cpu.eip()), 0x201);
    assert_eq!(
        cpu.with(|context| context.cpu.segment(relocade_x86::Segment::Cs).selector),
        0
    );
    assert_eq!(input.consumed.get(), 1);
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted); // Resume call, BIOS IRET.
    assert_eq!(cpu.with(|context| context.cpu.ax()), 30);
    assert_eq!(cpu.with(|context| context.cpu.sp()), 0x8000);
    assert_eq!(cpu.with(|context| context.cpu.eflags()), 0x243);
    assert_eq!(started.get(), 1);
    assert_eq!(completed.get(), 1);
    assert_eq!(dropped.get(), 1);
    assert_eq!(irq.borrow().acknowledged, 2);
}

async fn check_nested_calls() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut cpu = cpu(&irq, 0x200).await;
    let inputs = [0; 3].map(|_| Rc::new(Input::default()));
    let started = Rc::new(Cell::new(0));
    let finished = Rc::new(RefCell::new(Vec::new()));
    let dropped = Rc::new(Cell::new(0));
    let (sources, starts, finishes, drops) = (
        inputs.clone(),
        started.clone(),
        finished.clone(),
        dropped.clone(),
    );
    cpu.install_hooks([Hook::new(0x200, 0x201, async move |call| {
        let index = starts.get();
        starts.set(index + 1);
        let _operation = Operation(drops.clone());
        let value = call.with_interrupts_enabled(sources[index].read()).await;
        finishes.borrow_mut().push((index, value));
        call.with(|context| {
            assert!(!context.cpu.interrupts_enabled());
            real_mode_interrupt::return_from_interrupt(&mut context.cpu)
        })
    })])
    .await
    .unwrap();
    pending(cpu.run(20));
    irq.borrow_mut().raise();
    pending(cpu.run(20));
    assert_eq!(
        started.get(),
        2,
        "reentering the same service creates a nested invocation"
    );
    inputs[0].send(10);
    pending(cpu.run(20));
    assert_eq!(inputs[0].consumed.get(), 0, "outer future stays parked");
    irq.borrow_mut().raise();
    pending(cpu.run(20));
    assert_eq!(started.get(), 3);
    inputs[1].send(20);
    pending(cpu.run(20));
    assert_eq!(
        inputs[1].consumed.get(),
        0,
        "middle future also stays parked"
    );
    inputs[2].send(30);
    assert_eq!(cpu.run(10).await, ExecutionExit::BudgetExhausted);
    assert_eq!(*finished.borrow(), [(2, 30), (1, 20), (0, 10)]);
    assert_eq!(started.get(), 3);
    assert_eq!(dropped.get(), 3);
    assert_eq!(cpu.with(|context| context.cpu.sp()), 0x8000);
}

async fn check_if_scopes() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut cpu = cpu(&irq, 0x300).await;
    let input = Rc::new(Input::default());
    let dropped = Rc::new(Cell::new(0));
    let (source, drops) = (input.clone(), dropped.clone());
    cpu.install_hooks([Hook::new(0x200, 0x201, async move |call| {
        let _operation = Operation(drops.clone());
        for flags in [0x443, 0x643] {
            call.with(|context| context.cpu.set_eflags(flags));
            call.with_interrupts_enabled(async {
                call.with(|context| context.cpu.set_interrupts_enabled(false));
                call.with_interrupts_enabled(async {
                    call.with(|context| {
                        assert!(context.cpu.interrupts_enabled());
                        context.cpu.set_carry(false);
                    });
                })
                .await;
                call.with(|context| assert!(context.cpu.interrupts_enabled()));
            })
            .await;
            call.with(|context| assert_eq!(context.cpu.eflags(), flags & !1));

            let mut first = Box::pin(call.with_interrupts_enabled(std::future::pending::<()>()));
            let mut second = Box::pin(call.with_interrupts_enabled(std::future::pending::<()>()));
            poll_fn(|cx| {
                assert!(first.as_mut().poll(cx).is_pending());
                assert!(second.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            drop(first);
            call.with(|context| assert!(context.cpu.interrupts_enabled()));
            drop(second);
            call.with(|context| assert_eq!(context.cpu.eflags(), flags & !1));
        }
        call.with_interrupts_enabled(async {
            // A live helper does not grant IRQ permission when IF is clear.
            call.with(|context| context.cpu.set_interrupts_enabled(false));
            source.read().await;
        })
        .await;
        source.read().await; // IF was restored to true; no helper is needed.
        HookResult::Exit(ExecutionExit::Host(HostExit::Return))
    })])
    .await
    .unwrap();
    pending(cpu.run(5));
    irq.borrow_mut().raise();
    pending(cpu.run(5));
    assert_eq!(irq.borrow().acknowledged, 0);
    input.send(1);
    pending(cpu.run(20));
    assert_eq!(irq.borrow().acknowledged, 1);
    assert_eq!(dropped.get(), 0);
    drop(cpu);
    assert_eq!(dropped.get(), 1, "CPU drop releases the retained future");
}

async fn check_completed_call_scope() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut cpu = cpu(&irq, 0x300).await;
    cpu.install_hooks([Hook::new(
        0x200,
        0x201,
        |call: relocade_machine::CallContext| {
            let mut wait = Box::pin(async move {
                call.with_interrupts_enabled(std::future::pending::<()>())
                    .await;
            });
            // poll_fn retains its captures after returning Ready, until it is dropped.
            poll_fn(move |cx| {
                assert!(wait.as_mut().poll(cx).is_pending());
                Poll::Ready(HookResult::Exit(ExecutionExit::Host(HostExit::Return)))
            })
        },
    )])
    .await
    .unwrap();
    assert_eq!(cpu.run(5).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(
        cpu.with(|context| context.cpu.eflags()),
        0x43,
        "restore live IF on completion"
    );
}

async fn check_abandoned_call() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut cpu = cpu(&irq, 0x300).await;
    let dropped = Rc::new(Cell::new(0));
    let drops = dropped.clone();
    cpu.install_hooks([
        Hook::new(0x200, 0x201, async move |call| {
            let _operation = Operation(drops.clone());
            call.with(|context| context.cpu.set_interrupts_enabled(true));
            call.with_interrupts_enabled(std::future::pending::<()>())
                .await;
            unreachable!()
        }),
        Hook::new(0x300, 0x301, async |_| {
            HookResult::Exit(ExecutionExit::Host(HostExit::Return))
        }),
    ])
    .await
    .unwrap();
    pending(cpu.run(5));
    irq.borrow_mut().raise();
    assert_eq!(cpu.run(5).await, ExecutionExit::Host(HostExit::Return));
    assert!(
        !cpu.with(|context| context.cpu.interrupts_enabled()),
        "keep the ISR's live IF"
    );
    assert_eq!(dropped.get(), 1, "session return releases the parked call");
    drop(cpu);
    assert_eq!(dropped.get(), 1);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn call_contexts_cannot_access_a_parked_or_destroyed_cpu() {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    pollster::block_on(async {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let mut cpu = cpu(&irq, 0x300).await;
        let context = Rc::new(RefCell::new(None));
        let retained_context = context.clone();
        let owner = Rc::new(());
        let released = Rc::downgrade(&owner);
        cpu.install_hooks([Hook::new(0x200, 0x201, async move |call| {
            call.with(|context| assert_eq!(context.cpu.sp(), 0x7ffa));
            let call = Rc::new(call);
            *retained_context.borrow_mut() = Some(call.clone());
            let _owner = owner.clone();
            call.with_interrupts_enabled(std::future::pending::<()>())
                .await;
            unreachable!()
        })])
        .await
        .unwrap();
        pending(cpu.run(20));
        let call = context.borrow_mut().take().unwrap();
        assert!(catch_unwind(AssertUnwindSafe(|| call.with(|_| ()))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| call.cpu.ax())).is_err());
        irq.borrow_mut().raise();
        assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
        assert_eq!(cpu.with(|context| context.cpu.eip()), 0x306);
        assert!(catch_unwind(AssertUnwindSafe(|| call.with(|_| ()))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| call.cpu.ax())).is_err());
        let memory = call.memory.view().with_io(&call.io);
        drop(cpu);
        memory.read_u16(0x500).unwrap();
        assert!(
            released.upgrade().is_none(),
            "escaped contexts do not retain the CPU"
        );
        assert!(catch_unwind(AssertUnwindSafe(|| call.with(|_| ()))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| call.cpu.ax())).is_err());
    });
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn plain_pending_await_retains_locals_across_irqs_and_budget_exits() {
    pollster::block_on(check_retained_future());
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn nested_calls_at_the_same_hook_resume_in_stack_order() {
    pollster::block_on(check_nested_calls());
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn if_scopes_restore_if_on_completion_and_cancellation() {
    pollster::block_on(check_if_scopes());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn completed_calls_restore_if_from_retained_waits() {
    pollster::block_on(check_completed_call_scope());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn session_return_abandons_parked_calls() {
    pollster::block_on(check_abandoned_call());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn plain_pending_await_retains_locals_across_irqs_and_budget_exits() {
    check_retained_future().await;
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn nested_calls_at_the_same_hook_resume_in_stack_order() {
    check_nested_calls().await;
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn if_scopes_restore_if_on_completion_and_cancellation() {
    check_if_scopes().await;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn completed_calls_restore_if_from_retained_waits() {
    check_completed_call_scope().await;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn session_return_abandons_parked_calls() {
    check_abandoned_call().await;
}

#[path = "runtime_calls/resumption.rs"]
mod resumption;
