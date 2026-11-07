use super::{cpu, pending, support, Input, Irq, Operation};
use relocade_machine::{real_mode_interrupt::Interrupts, FarPointer};
use relocade_x86::{Exception, ExecutionExit, HostExit};
use std::{
    cell::{Cell, RefCell},
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    task::Poll,
};

const SERVICE: FarPointer = FarPointer {
    segment: 0,
    offset: 0x200,
};

async fn nested_calls() {
    for abandon in [false, true] {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let mut runner = cpu(&irq, 0x300).await;
        let state = runner.cpu_state();
        let memory = runner.memory();
        memory.view().write_u32(0x61 * 4, 0x400).unwrap();
        memory
            .view()
            .write(
                0x400,
                &[
                    0x8b, 0x0e, 0,
                    5, // Record the IRQ count before entering the inner service.
                    0xcd, 0x22, 0x40, 0xcf, // INT 22h; INC AX; IRET.
                ],
            )
            .unwrap();
        let input = Rc::new(Input::default());
        let finished = Rc::new(RefCell::new(Vec::new()));
        let dropped = Rc::new(Cell::new(0));
        let mut interrupts = Interrupts::new(&mut runner);
        interrupts.register_interrupt(0x21, SERVICE, {
            let (irq, finished, dropped) = (irq.clone(), finished.clone(), dropped.clone());
            async move |call| {
                let _operation = Operation(dropped.clone());
                call.cpu.set_interrupts_enabled(true);
                irq.borrow_mut().raise();
                call.call_interrupt(0x61).await;
                finished.borrow_mut().push("outer");
            }
        });
        interrupts.register_interrupt(0x22, SERVICE.wrapping_add(0x20), {
            let (input, finished, dropped) = (input.clone(), finished.clone(), dropped.clone());
            async move |call| {
                let _operation = Operation(dropped.clone());
                call.cpu.set_interrupts_enabled(true);
                input.read().await;
                call.call_interrupt(0x62).await;
                finished.borrow_mut().push("inner");
                call.cpu.set_interrupts_enabled(false);
            }
        });
        // A Rust caller uses the same operation for a Rust implementation.
        interrupts.register_interrupt(
            0x62,
            FarPointer {
                segment: 0,
                offset: 0x600,
            },
            async |call| {
                call.cpu.set_bx(call.cpu.bx().wrapping_add(1));
            },
        );
        interrupts.install().await.unwrap();

        pending(runner.run());
        assert_eq!(irq.borrow().acknowledged, 1);
        assert_eq!(memory.view().read_u16(0x500).unwrap(), 1);
        assert_eq!(
            state.cx(),
            0,
            "the explicit call enters before the queued IRQ"
        );
        assert!(
            finished.borrow().is_empty(),
            "IRQ return does not finish the outer call"
        );
        pending(runner.run()); // Replacing the waiter retains both invocations.
        assert_eq!(dropped.get(), 0);

        if abandon {
            state.set_ax(0x1234);
            runner.exit_handle().request();
            assert_eq!(runner.run().await, ExecutionExit::Host(HostExit::Return));
            assert!(finished.borrow().is_empty());
            assert_eq!(
                state.ax(),
                0x1234,
                "abandonment does not run completion code"
            );
        } else {
            input.send(7);
            assert_eq!(
                support::run_until(&mut runner, 0x102).await,
                ExecutionExit::Interrupted
            );
            assert_eq!(*finished.borrow(), ["inner", "outer"]);
            assert_eq!(state.ax(), 1);
            assert_eq!(state.bx(), 1);
            assert_eq!(state.sp(), 0x8000);
            assert_eq!(state.eflags(), 0x243);
        }
        assert_eq!(dropped.get(), 2);
    }
}

async fn faults() {
    for entry_fault in [true, false] {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let mut runner = cpu(&irq, 0x300).await;
        let state = runner.cpu_state();
        let memory = runner.memory();
        if !entry_fault {
            memory
                .view()
                .write(0x300, &[0x67, 0xa1, 0, 0, 1, 0, 0x40, 0xcf])
                .unwrap(); // MOV AX,[10000] exceeds DS; INC AX; IRET.
        }
        let started = Rc::new(Cell::new(0));
        let finished = Rc::new(Cell::new(0));
        let mut interrupts = Interrupts::new(&mut runner);
        interrupts.register_interrupt(0x21, SERVICE, {
            let (started, finished) = (started.clone(), finished.clone());
            async move |call| {
                started.set(started.get() + 1);
                if entry_fault {
                    call.cpu.set_sp(4); // The six-byte interrupt frame crosses the stack limit.
                }
                call.call_interrupt(0x60).await;
                finished.set(finished.get() + 1);
            }
        });
        interrupts.install().await.unwrap();
        let exception = if entry_fault {
            Exception::StackFault { error_code: 0 }
        } else {
            Exception::GeneralProtection { error_code: 0 }
        };
        assert_eq!(runner.run().await, ExecutionExit::Exception(exception));
        assert_eq!(started.get(), 1);
        assert_eq!(finished.get(), 0);
        if entry_fault {
            assert_eq!(state.eip(), 0x201);
            state.set_sp(0x7ffa);
        } else {
            assert_eq!(state.eip(), 0x300);
            state.set_eip(0x306); // Resume after the undelivered segment-limit fault.
        }
        assert_eq!(
            support::run_until(&mut runner, 0x102).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(
            started.get(),
            1,
            "recovery retains the same Rust invocation"
        );
        assert_eq!(finished.get(), 1);
        assert_eq!(state.sp(), 0x8000);
        if entry_fault {
            assert_eq!(memory.view().read_u16(0x500).unwrap(), 1);
        } else {
            assert_eq!(state.ax(), 1);
        }
    }
}

async fn canceled_requests() {
    for replace in [false, true] {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let mut runner = cpu(&irq, 0x300).await;
        let memory = runner.memory();
        memory.view().write_u32(0x61 * 4, 0x400).unwrap();
        memory.view().write(0x400, &[0x0f, 0x0b]).unwrap(); // A canceled callback must never execute.
        let mut interrupts = Interrupts::new(&mut runner);
        interrupts.register_interrupt(0x21, SERVICE, async move |call| {
            {
                let mut invocation = pin!(call.call_interrupt(0x61));
                poll_fn(|cx| {
                    assert!(invocation.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;
            }
            // Both paths finish their initial poll with a canceled request:
            // either replace it immediately or return without another await.
            if replace {
                call.call_interrupt(0x60).await;
            }
            call.cpu.set_ax(7);
        });
        interrupts.install().await.unwrap();
        assert_eq!(
            support::run_until(&mut runner, 0x102).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(runner.cpu_state().ax(), 7);
        assert_eq!(runner.cpu_state().sp(), 0x8000);
        assert_eq!(memory.view().read_u16(0x500).unwrap(), u16::from(replace));
    }
}

test_backends!(
    interrupt_calls_nest_across_irqs_waiter_replacement_and_abandonment,
    nested_calls
);
test_backends!(interrupt_call_faults_retain_the_waiting_handler, faults);
test_backends!(
    canceled_interrupt_calls_do_not_enter_the_handler,
    canceled_requests
);
