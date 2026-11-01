#[path = "support/interrupts.rs"]
mod interrupts;
mod support;

use interrupts::Irq;
use relocade_machine::{real_mode_interrupt, Cpu, DeviceConnection, HookResult, IoBus, Memory};
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

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

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

struct Operation(Rc<Cell<usize>>);
impl Drop for Operation {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

async fn cpu(irq: &Rc<RefCell<Irq>>, handler: u16) -> Cpu {
    let mut state = CpuState {
        eip: 0x100,
        segments: Segments::real_mode(),
        ..Default::default()
    };
    state.registers.esp = 0x8000;
    state.flags.set_eflags(0x243);
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        state,
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let memory = Memory::new(config.machine).unwrap();
    {
        let mut view = memory.view();
        view.write(0x100, &[0xcd, 0x21, 0xeb, 0xfe]).unwrap();
        view.write(0x84, &[0, 2, 0, 0]).unwrap();
        view.write(0x180, &[handler as u8, (handler >> 8) as u8, 0, 0])
            .unwrap();
        view.write(0x200, &real_mode_interrupt::HOOK_STUB).unwrap();
        // INC word [0500]; JMP 0306; IRET. The jump creates an ISR budget boundary.
        view.write(0x300, &[0xff, 0x06, 0, 5, 0xeb, 0, 0xcf])
            .unwrap();
    }
    Cpu::new(
        memory,
        config.cpu,
        IoBus::new([DeviceConnection {
            device: irq.clone(),
            ports: vec![],
            mmio: vec![],
        }]),
    )
    .await
    .unwrap()
}

fn pending(future: impl Future) {
    assert!(pin!(future)
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
}

async fn check_retained_future() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    let cpu = runner.cpu_state();
    let input = Rc::new(Input::default());
    input.send(10);
    let started = Rc::new(Cell::new(0));
    let completed = Rc::new(Cell::new(0));
    let dropped = Rc::new(Cell::new(0));
    runner
        .install_hooks([real_mode_interrupt::hook(0x200, {
            let (cpu, source, starts, finishes, drops) = (
                cpu.clone(),
                input.clone(),
                started.clone(),
                completed.clone(),
                dropped.clone(),
            );
            async move || {
                starts.set(starts.get() + 1);
                let _operation = Operation(drops.clone());
                cpu.set_interrupts_enabled(true);
                let first = source.read().await;
                let local = [first];
                let saved = &local[0];
                // An ordinary await and live IF suffice. The borrowed local stays pinned.
                let second = source.read().await;
                finishes.set(finishes.get() + 1);
                assert_eq!(cpu.eflags(), 0x243);
                assert_eq!(cpu.eip(), 0x200, "restore only the original code position");
                cpu.set_ax(*saved + second);
                HookResult::Complete
            }
        })])
        .await
        .unwrap();
    {
        let mut run = pin!(runner.run(20));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(run.as_mut().poll(&mut cx).is_pending());
        irq.borrow_mut().raise();
        assert!(run.as_mut().poll(&mut cx).is_pending());
    }
    assert_eq!(runner.memory().view().read_u16(0x500).unwrap(), 1);
    assert_eq!(started.get(), 1);
    assert_eq!(input.consumed.get(), 1);
    assert_eq!(completed.get(), 0);
    assert_eq!(dropped.get(), 0);

    irq.borrow_mut().raise();
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runner.context().cpu.eip(), 0x306);
    assert_eq!(runner.memory().view().read_u16(0x500).unwrap(), 2);
    let mut frame = [0; 6];
    runner.memory().view().read(0x7ff4, &mut frame).unwrap();
    assert_eq!(
        frame,
        [2, 2, 0, 0, 0x43, 2],
        "IRQ return names the service resume position"
    );
    input.send(20);
    assert_eq!(runner.run(0).await, ExecutionExit::BudgetExhausted);
    assert_eq!(input.consumed.get(), 1);
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted); // IRQ IRET only.
    assert_eq!(cpu.eip(), 0x202);
    assert_eq!(
        input.consumed.get(),
        1,
        "ready input cannot resume a parked future"
    );
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted); // Resume, then service IRET.
    assert_eq!(cpu.ax(), 30);
    assert_eq!(cpu.sp(), 0x8000);
    assert_eq!(cpu.eflags(), 0x243);
    assert_eq!((started.get(), completed.get(), dropped.get()), (1, 1, 1));
    assert_eq!(irq.borrow().acknowledged, 2);
}

async fn check_nested_calls() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x200).await;
    let cpu = runner.cpu_state();
    let inputs = [0; 3].map(|_| Rc::new(Input::default()));
    let started = Rc::new(Cell::new(0));
    let finished = Rc::new(RefCell::new(Vec::new()));
    let dropped = Rc::new(Cell::new(0));
    runner
        .install_hooks([real_mode_interrupt::hook(0x200, {
            let (cpu, sources, starts, finishes, drops) = (
                cpu.clone(),
                inputs.clone(),
                started.clone(),
                finished.clone(),
                dropped.clone(),
            );
            async move || {
                let index = starts.get();
                starts.set(index + 1);
                let _operation = Operation(drops.clone());
                let value =
                    real_mode_interrupt::with_interrupts_enabled(&cpu, sources[index].read()).await;
                finishes.borrow_mut().push((index, value));
                assert!(!cpu.interrupts_enabled());
                HookResult::Complete
            }
        })])
        .await
        .unwrap();
    pending(runner.run(20));
    irq.borrow_mut().raise();
    pending(runner.run(20));
    assert_eq!(started.get(), 2);
    inputs[0].send(10);
    pending(runner.run(20));
    assert_eq!(inputs[0].consumed.get(), 0);
    irq.borrow_mut().raise();
    pending(runner.run(20));
    assert_eq!(started.get(), 3);
    inputs[1].send(20);
    pending(runner.run(20));
    assert_eq!(inputs[1].consumed.get(), 0);
    inputs[2].send(30);
    assert_eq!(runner.run(10).await, ExecutionExit::BudgetExhausted);
    assert_eq!(*finished.borrow(), [(2, 30), (1, 20), (0, 10)]);
    assert_eq!(started.get(), 3);
    assert_eq!(dropped.get(), 3);
    assert_eq!(cpu.sp(), 0x8000);
}

async fn check_live_if_and_ready_awaits() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    let input = Rc::new(Input::default());
    runner
        .install_hooks([real_mode_interrupt::hook(0x200, {
            let cpu = runner.cpu_state();
            let source = input.clone();
            let device = irq.clone();
            async move || {
                assert!(!cpu.interrupts_enabled());
                source.read().await;
                assert_eq!(device.borrow().acknowledged, 0);
                cpu.set_interrupts_enabled(true);
                source.read().await;
                assert_eq!(device.borrow().acknowledged, 1);
                device.borrow_mut().raise();
                cpu.set_ax(std::future::ready(7).await);
                assert_eq!(
                    device.borrow().acknowledged,
                    1,
                    "ready awaits keep executing"
                );
                HookResult::Exit(ExecutionExit::Host(HostExit::Return))
            }
        })])
        .await
        .unwrap();
    pending(runner.run(10));
    irq.borrow_mut().raise();
    pending(runner.run(10));
    assert_eq!(irq.borrow().acknowledged, 0);
    input.send(1);
    pending(runner.run(10));
    assert_eq!(irq.borrow().acknowledged, 1);
    input.send(2);
    assert_eq!(runner.run(10).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(runner.cpu_state().ax(), 7);
    assert_eq!(irq.borrow().acknowledged, 1);
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
    plain_pending_await_retains_locals_across_irqs_and_budget_exits,
    check_retained_future
);
test_backends!(
    nested_calls_at_the_same_hook_resume_in_stack_order,
    check_nested_calls
);
test_backends!(
    pending_awaits_follow_live_if_and_ready_awaits_finish_synchronously,
    check_live_if_and_ready_awaits
);
