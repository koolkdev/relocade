use super::{cpu, Irq};
use relocade_machine::{Cpu, DispatchEntry, Hook};
use relocade_x86::{Compiler, Exception, ExecutionExit, ExecutionProfile, HostExit};
use std::{cell::RefCell, rc::Rc};

async fn one_instruction(cpu: &mut Cpu, code: &[u8], compiled: bool) {
    if compiled {
        let module = Compiler::new(ExecutionProfile::Real16)
            .compile_block(0x100, code, 1)
            .unwrap();
        cpu.install_module(
            &module.bytes,
            &[DispatchEntry {
                address: 0x100,
                export: &module.entry,
            }],
        )
        .await
        .unwrap();
    } else {
        cpu.install_module(
            relocade_interpreter::module(ExecutionProfile::Real16),
            &[DispatchEntry {
                address: 0x100,
                export: "step",
            }],
        )
        .await
        .unwrap();
    }
}

async fn protected_successor() {
    for compiled in [false, true] {
        for (instruction, flags, inhibited) in [
            (&[0xfb][..], 2, true),
            (&[0xfb][..], 0x202, false),
            (&[0x8e, 0xd0][..], 0x202, true),
            (&[0x17][..], 0x202, true),
            (&[0x0f, 0xb2, 0x06, 0, 8][..], 0x202, false),
        ] {
            let irq = Rc::new(RefCell::new(Irq::default()));
            // OUT raises an IRQ while executing as the protected successor.
            let code = [instruction, &[0xe6, 0x80, 0xeb, 0xfe]].concat();
            let mut runner = cpu(&code, flags, &irq).await;
            one_instruction(&mut runner, &code, compiled).await;
            assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
            let state = runner.cpu_state();
            assert_eq!(state.instruction_count(), if inhibited { 2 } else { 1 });
            assert_eq!(
                state.eip(),
                0x100 + instruction.len() as u32 + if inhibited { 2 } else { 0 }
            );
            assert_eq!(irq.borrow().acknowledged, 0);
            if !inhibited {
                irq.borrow_mut().raise();
            }
            assert_eq!(runner.run(0).await, ExecutionExit::BudgetExhausted);
            assert_eq!(irq.borrow().acknowledged, 0);
            assert_eq!(runner.run(2).await, ExecutionExit::BudgetExhausted);
            assert_eq!(irq.borrow().acknowledged, 1);
            assert_eq!(runner.memory().view().read_u16(0x500).unwrap(), 1);
        }
    }
}

async fn consecutive_inhibitors() {
    for compiled in [false, true] {
        for (instructions, flags) in [
            (&[0x8e, 0xd0, 0x8e, 0xd0][..], 0x202), // MOV SS; MOV SS.
            (&[0x8e, 0xd0, 0xfb][..], 2),           // MOV SS; STI.
            (&[0xfb, 0x8e, 0xd0][..], 2),           // STI; MOV SS.
        ] {
            let irq = Rc::new(RefCell::new(Irq::default()));
            let code = [instructions, &[0xe6, 0x80, 0xeb, 0xfe]].concat();
            let mut runner = cpu(&code, flags, &irq).await;
            one_instruction(&mut runner, &code, compiled).await;
            assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
            assert_eq!(runner.cpu_state().instruction_count(), 3);
            assert_eq!(
                runner.cpu_state().eip(),
                0x100 + instructions.len() as u32 + 2
            );
            assert_eq!(irq.borrow().acknowledged, 0);
            assert_eq!(runner.run(2).await, ExecutionExit::BudgetExhausted);
            assert_eq!(irq.borrow().acknowledged, 1);
        }
        // STI with IF already set does not request another protected instruction.
        let irq = Rc::new(RefCell::new(Irq::default()));
        let code = [0xfb, 0xfb, 0xe6, 0x80, 0xeb, 0xfe];
        let mut runner = cpu(&code, 2, &irq).await;
        one_instruction(&mut runner, &code, compiled).await;
        assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
        assert_eq!(runner.cpu_state().instruction_count(), 2);
        assert_eq!(runner.cpu_state().eip(), 0x102);
    }
}

async fn faults() {
    for compiled in [false, true] {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let code = [0x8e, 0x16, 0xff, 0xff]; // MOV SS,[FFFF] exceeds DS.
        let mut runner = cpu(&code, 0x202, &irq).await;
        one_instruction(&mut runner, &code, compiled).await;
        let before = runner.cpu_state().snapshot();
        assert_eq!(
            runner.run(1).await,
            ExecutionExit::Exception(Exception::GeneralProtection { error_code: 0 })
        );
        assert_eq!(runner.cpu_state().snapshot(), before);

        for (successor, fault) in [
            (
                &[0x67, 0xa1, 0, 0, 1, 0][..],
                ExecutionExit::Exception(Exception::GeneralProtection { error_code: 0 }),
            ),
            (
                &[0xf4][..],
                ExecutionExit::UnsupportedInstruction {
                    eip: 0x101,
                    opcode: 0xf4,
                },
            ),
        ] {
            let irq = Rc::new(RefCell::new(Irq::default()));
            let code = [&[0xfb][..], successor].concat();
            let mut runner = cpu(&code, 2, &irq).await;
            one_instruction(&mut runner, &code, compiled).await;
            irq.borrow_mut().raise();
            assert_eq!(
                runner.run(1).await,
                fault,
                "the protected step precedes the budget exit"
            );
            assert_eq!(runner.cpu_state().instruction_count(), 1);
            assert_eq!(runner.cpu_state().eip(), 0x101);
            assert!(runner.cpu_state().interrupts_enabled());
            assert_eq!(irq.borrow().acknowledged, 0);
        }
    }
}

async fn software_interrupt() {
    for compiled in [false, true] {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let code = [0xfb, 0xcd, 0x21, 0xeb, 0xfe]; // STI; INT 21h.
        let mut runner = cpu(&code, 2, &irq).await;
        runner.memory().view().write(0x84, &[0, 2, 0, 0]).unwrap();
        one_instruction(&mut runner, &code, compiled).await;
        runner
            .install_hooks([Hook::new(0x200, {
                let state = runner.cpu_state();
                let memory = runner.memory();
                let exit = runner.exit_handle();
                async move || {
                    assert_eq!(state.instruction_count(), 2);
                    assert!(!state.interrupts_enabled());
                    assert_eq!(state.sp(), 0x7ffa);
                    let mut frame = [0; 6];
                    memory.view().read(0x7ffa, &mut frame).unwrap();
                    assert_eq!(frame, [3, 1, 0, 0, 2, 2]);
                    exit.request();
                }
            })])
            .await
            .unwrap();
        irq.borrow_mut().raise();
        // Budget one still executes the protected INT, then exits before dispatching its target.
        assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
        assert_eq!(runner.cpu_state().eip(), 0x200);
        assert_eq!(irq.borrow().acknowledged, 0);
        assert_eq!(runner.run(1).await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(irq.borrow().acknowledged, 0);
    }
}

async fn function_call() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    // STI; CALL 0200; JMP to self. CALL is the protected instruction.
    let mut runner = cpu(&[0xfb, 0xe8, 0xfc, 0, 0xeb, 0xfe], 2, &irq).await;
    runner
        .install_hooks([Hook::new(0x200, {
            let state = runner.cpu_state();
            let memory = runner.memory();
            let exit = runner.exit_handle();
            async move || {
                assert_eq!(state.instruction_count(), 2);
                assert!(state.interrupts_enabled());
                assert_eq!(state.sp(), 0x7ffe);
                assert_eq!(memory.view().read_u16(0x7ffe).unwrap(), 0x104);
                exit.request();
            }
        })])
        .await
        .unwrap();
    assert_eq!(runner.run(10).await, ExecutionExit::Host(HostExit::Return));
}

test_backends!(
    protected_successors_execute_before_budget_exits,
    protected_successor
);
test_backends!(
    consecutive_inhibitors_use_the_same_step_loop,
    consecutive_inhibitors
);
test_backends!(
    protected_instruction_faults_publish_completed_effects,
    faults
);
test_backends!(
    software_interrupts_execute_as_protected_successors,
    software_interrupt
);
test_backends!(
    calls_dispatch_their_host_target_after_the_protected_instruction,
    function_call
);
