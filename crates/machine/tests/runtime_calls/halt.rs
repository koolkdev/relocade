use super::{cpu, pending, Input, Irq, Operation};
use relocade_machine::real_mode_interrupt;
use relocade_x86::{ExecutionExit, HostExit};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

async fn halted_isr() {
    for cancel in [false, true] {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let mut cpu = cpu(&irq, 0x300).await;
        let state = cpu.cpu_state();
        let memory = cpu.memory();
        memory.view().write(0x300, &[0xfb, 0xf4, 0xcf]).unwrap(); // STI; HLT; IRET.
        let input = Rc::new(Input::default());
        let drops = Rc::new(Cell::new(0));
        cpu.install(real_mode_interrupt::hook(0x200, state.clone(), {
            let state = state.clone();
            let input = input.clone();
            let drops = drops.clone();
            let exit = cpu.exit_handle();
            async move || {
                let _operation = Operation(drops.clone());
                state.set_ax(
                    real_mode_interrupt::with_interrupts_enabled(&state, input.read()).await,
                );
                exit.request();
            }
        }))
        .await
        .unwrap();
        pending(cpu.run());
        irq.borrow_mut().raise();
        pending(cpu.run());
        assert_eq!(state.eip(), 0x302);
        assert_eq!(state.instruction_count(), 3); // INT 21h; STI; HLT.
        assert!(state.interrupts_enabled());
        let halted = state.snapshot();
        input.send(42);
        pending(cpu.run());
        assert_eq!(state.snapshot(), halted);
        assert_eq!(
            input.consumed.get(),
            0,
            "ready outer input cannot release an ISR's HLT"
        );
        assert_eq!(drops.get(), 0);
        if cancel {
            drop(cpu);
            assert_eq!(
                state.snapshot(),
                halted,
                "abandonment cannot restore the outer call's IF"
            );
        } else {
            // A second ISR returns to the instruction after HLT; only the first
            // ISR's IRET may then reach the outer service's resume entry.
            memory.view().write(0x180, &[0, 4, 0, 0]).unwrap();
            memory.view().write(0x400, &[0xcf]).unwrap();
            irq.borrow_mut().raise();
            assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
            assert_eq!(input.consumed.get(), 1);
            assert_eq!(state.ax(), 42);
            assert_eq!(state.instruction_count(), 5);
            assert_eq!(irq.borrow().acknowledged, 2);
        }
        assert_eq!(drops.get(), 1);
    }
}

test_backends!(
    halted_isr_keeps_ready_outer_work_parked_until_guest_resume,
    halted_isr
);
