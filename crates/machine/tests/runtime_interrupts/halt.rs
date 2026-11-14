use super::{cpu, Irq};
use relocade_machine::DispatchEntry;
use relocade_x86::{Compiler, Exception, ExecutionExit, ExecutionProfile, HostExit};
use std::{
    cell::RefCell,
    future::Future,
    pin::pin,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Wake, Waker},
};

#[derive(Default)]
struct WakeCount(AtomicUsize);
impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn pending(future: impl Future, waker: &Waker) {
    assert!(pin!(future)
        .as_mut()
        .poll(&mut Context::from_waker(waker))
        .is_pending());
}

async fn waiting() {
    // Interpreter run, compiled HLT, and public instruction stepping all retain halt.
    for entry in 0..3 {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let code = [0xf4, 0x40, 0xf4, 0x90]; // HLT; INC AX; HLT; NOP.
        let mut cpu = cpu(&code, 2, &irq).await;
        if entry == 1 {
            let module = Compiler::new(ExecutionProfile::Real16)
                .compile_block(0x100, &code, 8)
                .unwrap();
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
        let interrupt = cpu.interrupt_handle();
        let state = cpu.cpu_state();
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        interrupt.request();
        // Reaching the instruction target must retain HLT before yielding.
        cpu.instruction_target().set(Some(1));
        if entry == 2 {
            assert_eq!(cpu.step(), ExecutionExit::Halted);
        } else {
            pending(cpu.run(), &waker);
        }
        assert_eq!(state.eip(), 0x101);
        assert_eq!(state.instruction_count(), 1);
        let halted = state.snapshot();
        irq.borrow_mut().raise();
        for _ in 0..3 {
            // Replacing the waiter and host notifications cannot release HLT.
            pending(cpu.run(), &waker);
            assert_eq!(cpu.step(), ExecutionExit::Halted);
            assert_eq!(state.snapshot(), halted);
        }
        assert_eq!(wakes.0.load(Ordering::Relaxed), 0, "HLT must not self-wake");
        assert_eq!(
            irq.borrow().acknowledged,
            0,
            "IF masks a queued IRQ during HLT"
        );
        assert!(
            interrupt.take(),
            "HLT does not consume the host notification"
        );

        state.set_interrupts_enabled(true);
        cpu.instruction_target().set(None);
        pending(cpu.run(), &waker);
        assert_eq!(irq.borrow().acknowledged, 1);
        assert_eq!(cpu.memory().view().read_u16(0x7ffa).unwrap(), 0x101);
        assert_eq!(cpu.memory().view().read_u16(0x500).unwrap(), 1);
        assert_eq!(state.ax(), 1);
        assert_eq!(state.eip(), 0x103);
        assert_eq!(state.instruction_count(), 5);

        // A terminal exit abandons this execution, allowing the CPU to be reused.
        cpu.exit_handle().request();
        assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(cpu.step(), ExecutionExit::StepComplete);
        assert_eq!(state.eip(), 0x104);
    }
}

async fn protected_halt() {
    for stepping in [false, true] {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let mut cpu = cpu(&[0xfb, 0xf4, 0xf4], 2, &irq).await; // STI; HLT; HLT.
        irq.borrow_mut().raise();
        let interrupt = cpu.interrupt_handle();
        interrupt.request();
        if stepping {
            assert_eq!(cpu.step(), ExecutionExit::Halted);
            assert_eq!(cpu.cpu_state().eip(), 0x102);
            assert_eq!(cpu.cpu_state().instruction_count(), 2);
            assert_eq!(irq.borrow().acknowledged, 0);
        }
        assert_eq!(cpu.run().await, ExecutionExit::Interrupted);
        assert_eq!(irq.borrow().acknowledged, 1);
        assert_eq!(cpu.memory().view().read_u16(0x7ffa).unwrap(), 0x102);
        assert_eq!(cpu.cpu_state().instruction_count(), 4);
        assert!(interrupt.take());
        pending(cpu.run(), Waker::noop());
        assert_eq!(cpu.cpu_state().eip(), 0x103);
        assert_eq!(cpu.cpu_state().instruction_count(), 5);
    }
}

async fn entry_fault() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut cpu = cpu(&[0xf4, 0x40, 0xf4], 0x202, &irq).await;
    pending(cpu.run(), Waker::noop());
    let state = cpu.cpu_state();
    state.set_sp(4);
    let before = state.snapshot();
    irq.borrow_mut().raise();
    assert_eq!(
        cpu.run().await,
        ExecutionExit::Exception(Exception::StackFault { error_code: 0 })
    );
    assert_eq!(state.snapshot(), before);
    assert_eq!(irq.borrow().acknowledged, 1);
    state.set_sp(0x8000);
    pending(cpu.run(), Waker::noop());
    assert_eq!(
        state.ax(),
        1,
        "accepted IRQ ends HLT even when entry faults"
    );
    assert_eq!(state.eip(), 0x103);
    assert_eq!(state.instruction_count(), 3);
}

test_backends!(
    halt_waits_for_an_eligible_irq_and_survives_replacement_waiters,
    waiting
);
test_backends!(
    sti_halt_retires_before_the_queued_irq_or_execution_interrupt,
    protected_halt
);
test_backends!(accepted_irq_ends_halt_even_when_entry_faults, entry_fault);
