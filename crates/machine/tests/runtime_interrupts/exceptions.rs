use super::{cpu, Irq};
use relocade_machine::{
    real_mode_interrupt::{read_frame, write_vector, Interrupts},
    DispatchEntry, FarPointer, Hook,
};
use relocade_x86::{Compiler, Exception, ExecutionExit, ExecutionProfile, HostExit};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const DEFAULT_HANDLER: FarPointer = FarPointer {
    segment: 0,
    offset: 0x700,
};

async fn live_vectors() {
    for compiled in [false, true] {
        for (instruction, exception) in [
            (&[0xf6, 0xf3][..], Exception::DivideError), // DIV BL, with BL=0.
            (&[0x62, 0x06, 0, 5][..], Exception::BoundRangeExceeded), // BOUND AX,[0500].
            (&[0x0f, 0x0b][..], Exception::InvalidOpcode), // UD2.
        ] {
            let irq = Rc::new(RefCell::new(Irq::default()));
            let code = [instruction, &[0xeb, 0xfe]].concat();
            let mut runner = cpu(&code, 0x40643, &irq).await;
            let state = runner.cpu_state();
            state.set_ax(1); // Outside BOUND's inclusive [0, 0] range.
            if compiled {
                let module = Compiler::new(ExecutionProfile::Real16)
                    .compile_block(0x100, &code, 1)
                    .unwrap();
                runner
                    .install_module(
                        &module.bytes,
                        &[DispatchEntry {
                            key: 0x100,
                            export: &module.entry,
                        }],
                    )
                    .await
                    .unwrap();
            }
            let mut interrupts = Interrupts::new(&mut runner);
            interrupts.register_unhandled_exception(exception, DEFAULT_HANDLER);
            interrupts.install().await.unwrap();
            let memory = runner.memory();
            // Guest software replaces the default and repairs its return IP.
            write_vector(
                &mut memory.view(),
                exception.vector() as u8,
                FarPointer {
                    segment: 0,
                    offset: 0x600,
                },
            );
            memory
                .view()
                .write(
                    0x600,
                    &[
                        0x55,
                        0x89,
                        0xe5, // PUSH BP; MOV BP,SP.
                        0x8b,
                        0x56,
                        2,
                        0x89,
                        0x16,
                        2,
                        5, // Copy saved IP to [0502].
                        0x83,
                        0x46,
                        2,
                        instruction.len() as u8, // ADD word [BP+2],length.
                        0x5d,
                        0xcf, // POP BP; IRET.
                    ],
                )
                .unwrap();
            runner
                .install([Hook::host(0x100 + instruction.len() as u32, {
                    let exit = runner.exit_handle();
                    async move || exit.request()
                })])
                .await
                .unwrap();
            assert_eq!(runner.run().await, ExecutionExit::Host(HostExit::Return));
            assert_eq!(memory.view().read_u16(0x502).unwrap(), 0x100);
            assert_eq!(state.sp(), 0x8000);
            assert_eq!(
                state.eflags(),
                0x643,
                "IRET restores FLAGS; entry cleared AC"
            );
            assert_eq!(irq.borrow().acknowledged, 0);
        }
    }
}

async fn default_handler() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&[0x0f, 0x0b], 0x40743, &irq).await;
    let state = runner.cpu_state();
    let mut interrupts = Interrupts::new(&mut runner);
    interrupts.register_unhandled_exception(Exception::InvalidOpcode, DEFAULT_HANDLER);
    interrupts.install().await.unwrap();
    // Stepping performs the same architectural entry and stops before its handler.
    assert_eq!(runner.step(), ExecutionExit::InterruptEntered);
    assert_eq!(state.eip(), u32::from(DEFAULT_HANDLER.offset));
    assert_eq!(state.instruction_count(), 0);
    assert_eq!(
        runner.run().await,
        ExecutionExit::UnhandledException(Exception::InvalidOpcode)
    );
    let frame = read_frame(&state, &runner.memory().view());
    assert_eq!(
        frame.return_address,
        FarPointer {
            segment: 0,
            offset: 0x100
        }
    );
    assert_eq!(frame.flags, 0x743);
    assert_eq!(state.sp(), 0x7ffa);
    assert_eq!(state.eip(), u32::from(DEFAULT_HANDLER.offset) + 2);
    assert_eq!(
        state.eflags(),
        0x443,
        "entry clears IF, TF and AC, preserving DF"
    );
    assert_eq!(state.instruction_count(), 0);
}

async fn fault_before_irq() {
    for compiled in [false, true] {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let mut runner = cpu(&[0xfb, 0xf6, 0xf3], 2, &irq).await; // STI; DIV BL.
        super::protected_steps::one_instruction(&mut runner, &[0xfb], compiled).await;
        let mut interrupts = Interrupts::new(&mut runner);
        interrupts.register_unhandled_exception(Exception::DivideError, DEFAULT_HANDLER);
        interrupts.install().await.unwrap();
        irq.borrow_mut().raise();
        assert_eq!(
            runner.run().await,
            ExecutionExit::UnhandledException(Exception::DivideError)
        );
        let frame = read_frame(&runner.cpu_state(), &runner.memory().view());
        assert_eq!(frame.return_address.offset, 0x101);
        assert_eq!(frame.flags, 0x202);
        assert_eq!(runner.cpu_state().instruction_count(), 1);
        assert_eq!(
            irq.borrow().acknowledged,
            0,
            "synchronous entry precedes IRQ polling"
        );
    }
}

async fn entry_failure() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&[0x0f, 0x0b], 2, &irq).await;
    runner.cpu_state().set_sp(4);
    let before = runner.cpu_state().snapshot();
    assert_eq!(
        runner.run().await,
        ExecutionExit::Exception(Exception::StackFault { error_code: 0 })
    );
    assert_eq!(runner.cpu_state().snapshot(), before);
}

async fn suspended_caller() {
    struct Operation(Rc<Cell<usize>>);
    impl Drop for Operation {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&[0xcd, 0x21], 2, &irq).await;
    runner.memory().view().write(0x300, &[0x0f, 0x0b]).unwrap();
    let dropped = Rc::new(Cell::new(0));
    let completed = Rc::new(Cell::new(false));
    let mut interrupts = Interrupts::new(&mut runner);
    interrupts.register_unhandled_exception(Exception::InvalidOpcode, DEFAULT_HANDLER);
    interrupts.register_interrupt(
        0x21,
        FarPointer {
            segment: 0,
            offset: 0x200,
        },
        {
            let dropped = dropped.clone();
            let completed = completed.clone();
            async move |call| {
                let _operation = Operation(dropped.clone());
                call.call_interrupt(0x60).await;
                completed.set(true);
            }
        },
    );
    interrupts.install().await.unwrap();
    assert_eq!(
        runner.run().await,
        ExecutionExit::UnhandledException(Exception::InvalidOpcode)
    );
    assert_eq!(dropped.get(), 1);
    assert!(!completed.get());
    assert_eq!(
        read_frame(&runner.cpu_state(), &runner.memory().view())
            .return_address
            .offset,
        0x300
    );
    // The terminal result was consumed, so a later requested return wins.
    runner.exit_handle().request();
    assert_eq!(runner.run().await, ExecutionExit::Host(HostExit::Return));
}

test_backends!(real_mode_faults_follow_guest_replaced_vectors, live_vectors);
test_backends!(
    default_exception_handlers_report_after_entry,
    default_handler
);
test_backends!(
    faulting_sti_successors_enter_before_pending_irqs,
    fault_before_irq
);
test_backends!(
    exception_entry_failures_return_without_redelivery,
    entry_failure
);
test_backends!(
    unhandled_exceptions_release_suspended_callers,
    suspended_caller
);
