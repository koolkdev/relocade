use super::{cpu, pending, Input, Irq, Operation};
use relocade_machine::{real_mode_interrupt, Hook, HookResult};
use relocade_x86::{Exception, ExecutionExit, HostExit, Segment};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

async fn guest_effects() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    let state = runner.cpu_state();
    let memory = runner.memory();
    memory
        .view()
        .write(
            0x300,
            &[
                0xb8, 0x20, 0, 0x8e, 0xd0, 0xbc, 0, 0x60, 0xeb,
                0, // SS=20, SP=6000; JMP next.
                0xb8, 0x30, 0, 0x8e, 0xd8, 0xbb, 0x67, 0x45, // DS=30, BX=4567.
                0x2e, 0xc7, 0x06, 0, 5, 0xfe, 0xca, // MOV word CS:[0500],CAFE.
                0x68, 0x46, 6, 0x6a, 0x10, 0x68, 2, 1,
                0xcf, // New FLAGS/CS/IP; IRET to 0010:0102.
            ],
        )
        .unwrap();
    let input = Rc::new(Input::default());
    runner
        .install_hooks([real_mode_interrupt::hook(0x200, {
            let cpu = state.clone();
            let memory = memory.clone();
            let source = input.clone();
            async move || {
                cpu.set_interrupts_enabled(true);
                assert_eq!(source.read().await, 7);
                assert_eq!(cpu.segment(Segment::Cs).selector, 0x10);
                assert_eq!(
                    cpu.eip(),
                    0x100,
                    "original code position through the live CS alias"
                );
                assert_eq!(cpu.ax(), 0x30);
                assert_eq!(cpu.bx(), 0x4567);
                assert_eq!(cpu.segment(Segment::Ds).selector, 0x30);
                assert_eq!(cpu.segment(Segment::Ss).selector, 0x20);
                assert_eq!(cpu.sp(), 0x6000);
                assert_eq!(cpu.eflags(), 0x646);
                assert_eq!(memory.view().read_u16(0x500).unwrap(), 0xcafe);
                HookResult::Exit(ExecutionExit::Host(HostExit::Return))
            }
        })])
        .await
        .unwrap();
    pending(runner.run(10));
    irq.borrow_mut().raise();
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
    input.send(7);
    assert_eq!(input.consumed.get(), 0);
    assert_eq!(runner.run(100).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(input.consumed.get(), 1);
}

async fn top_frame_resume() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x400).await;
    runner
        .memory()
        .view()
        .write(0x400, &real_mode_interrupt::HOOK_STUB)
        .unwrap();
    let inputs = [0; 2].map(|_| Rc::new(Input::default()));
    let starts = Rc::new(Cell::new(0));
    let finished = Rc::new(RefCell::new(Vec::new()));
    let state = runner.cpu_state();
    let hooks = [0x200, 0x400]
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let (cpu, source, starts, finished) = (
                state.clone(),
                inputs[index].clone(),
                starts.clone(),
                finished.clone(),
            );
            real_mode_interrupt::hook(entry, async move || {
                starts.set(starts.get() + 1);
                let value = real_mode_interrupt::with_interrupts_enabled(&cpu, source.read()).await;
                finished.borrow_mut().push((index, value));
                HookResult::Complete
            })
        });
    runner.install_hooks(hooks).await.unwrap();
    pending(runner.run(10));
    irq.borrow_mut().raise();
    pending(runner.run(10));
    assert_eq!(starts.get(), 2);
    runner.memory().view().write(0x180, &[0, 3, 0, 0]).unwrap();
    irq.borrow_mut().raise();
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
    inputs[0].send(10);
    inputs[1].send(20);
    // Return through the outer service's resume entry instead of the inner's.
    // The marker has no ID: the stack still resumes the inner invocation first.
    runner
        .memory()
        .view()
        .write_u16(u32::from(state.sp()), 0x202)
        .unwrap();
    assert_eq!(runner.run(10).await, ExecutionExit::BudgetExhausted);
    assert_eq!(*finished.borrow(), [(1, 20), (0, 10)]);
    assert_eq!(inputs[0].consumed.get(), 1);
    assert_eq!(inputs[1].consumed.get(), 1);
    assert_eq!(state.sp(), 0x8000);
    state.set_eip(0x202); // No suspended frame remains.
    let before = state.snapshot();
    assert!(matches!(
        runner.run(10).await,
        ExecutionExit::Host(HostExit::Resume)
    ));
    assert_eq!(state.snapshot(), before);
    assert_eq!(starts.get(), 2, "resume entries never invoke a handler");
}

async fn failed_entry() {
    for invalid_resume in [false, true] {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let mut runner = cpu(&irq, 0x300).await;
        let input = Rc::new(Input::default());
        let state = runner.cpu_state();
        let drops = Rc::new(Cell::new(0));
        runner
            .install_hooks([real_mode_interrupt::hook(0x200, {
                let (cpu, source, dropped) = (state.clone(), input.clone(), drops.clone());
                async move || {
                    let _operation = Operation(dropped.clone());
                    cpu.set_interrupts_enabled(true);
                    cpu.set_ax(source.read().await);
                    HookResult::Complete
                }
            })])
            .await
            .unwrap();
        pending(runner.run(10));
        if invalid_resume {
            // A valid real-mode CS whose base lies above the resume entry.
            state.set_segment(
                Segment::Cs,
                relocade_x86::SegmentRegister::real_mode(Segment::Cs, 0x21),
            );
        } else {
            state.set_sp(4);
        }
        let before = state.snapshot();
        irq.borrow_mut().raise();
        let fault = if invalid_resume {
            Exception::GeneralProtection { error_code: 0 }
        } else {
            Exception::StackFault { error_code: 0 }
        };
        assert_eq!(runner.run(10).await, ExecutionExit::Exception(fault));
        assert_eq!(
            state.snapshot(),
            before,
            "failed entry undoes only the temporary resume position"
        );
        assert_eq!(irq.borrow().acknowledged, 1);
        assert_eq!(drops.get(), 0);
        if invalid_resume {
            state.set_segment(
                Segment::Cs,
                relocade_x86::SegmentRegister::real_mode(Segment::Cs, 0),
            );
        } else {
            state.set_sp(0x7ffa);
        }
        input.send(42);
        assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
        assert_eq!(state.ax(), 42);
        assert_eq!(state.sp(), 0x8000);
        assert_eq!(drops.get(), 1);
    }
}

async fn isr_fault_and_repair() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    // A guest operand fault after successful IRQ entry is not an entry failure.
    runner
        .memory()
        .view()
        .write(0x300, &[0x67, 0xa1, 0, 0, 1, 0])
        .unwrap();
    let input = Rc::new(Input::default());
    let state = runner.cpu_state();
    runner
        .install_hooks([real_mode_interrupt::hook(0x200, {
            let (cpu, source) = (state.clone(), input.clone());
            async move || {
                cpu.set_ax(real_mode_interrupt::with_interrupts_enabled(&cpu, source.read()).await);
                HookResult::Complete
            }
        })])
        .await
        .unwrap();
    pending(runner.run(10));
    irq.borrow_mut().raise();
    assert_eq!(
        runner.run(10).await,
        ExecutionExit::Exception(Exception::GeneralProtection { error_code: 0 })
    );
    assert_eq!(state.eip(), 0x300);
    assert_eq!(state.sp(), 0x7ff4);
    assert!(!state.interrupts_enabled());
    input.send(42);
    assert_eq!(
        runner.run(10).await,
        ExecutionExit::Exception(Exception::GeneralProtection { error_code: 0 })
    );
    assert_eq!(
        input.consumed.get(),
        0,
        "ISR faults cannot poll the parked call"
    );
    runner.context().memory.write(0x300, &[0xcf]).unwrap();
    assert_eq!(runner.run(10).await, ExecutionExit::BudgetExhausted);
    assert_eq!(input.consumed.get(), 1);
    assert_eq!(state.ax(), 42);
    assert_eq!(state.sp(), 0x8000);
    assert_eq!(irq.borrow().acknowledged, 1);
}

async fn resume_position_fault() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    // Stop before editing the return alias to 0020:0002, then IRET to the resume
    // entry at 0202. That CS cannot represent the saved host position at 01FF.
    runner
        .memory()
        .view()
        .write(
            0x300,
            &[
                0xeb, 0, 0x89, 0xe5, 0xc7, 0x46, 0, 2, 0, 0xc7, 0x46, 2, 0x20, 0, 0xcf,
            ],
        )
        .unwrap();
    let input = Rc::new(Input::default());
    let state = runner.cpu_state();
    let drops = Rc::new(Cell::new(0));
    runner
        .install_hooks([real_mode_interrupt::hook(0x200, {
            let (cpu, source, dropped) = (state.clone(), input.clone(), drops.clone());
            async move || {
                let _operation = Operation(dropped.clone());
                cpu.set_eip(0x1ff);
                cpu.set_interrupts_enabled(true);
                cpu.set_ax(source.read().await);
                assert_eq!(cpu.eip(), 0x1ff);
                HookResult::Complete
            }
        })])
        .await
        .unwrap();
    pending(runner.run(10));
    irq.borrow_mut().raise();
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
    input.send(42);
    assert_eq!(
        runner.run(10).await,
        ExecutionExit::Exception(Exception::GeneralProtection { error_code: 0 })
    );
    assert_eq!(state.eip(), u32::MAX);
    assert_eq!(state.segment(Segment::Cs).selector, 0x20);
    assert_eq!(state.sp(), 0x7ffa);
    assert_eq!(input.consumed.get(), 0);
    assert_eq!(drops.get(), 0);
    state.set_segment(
        Segment::Cs,
        relocade_x86::SegmentRegister::real_mode(Segment::Cs, 0),
    );
    state.set_eip(0x202);
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(input.consumed.get(), 1);
    assert_eq!(drops.get(), 1);
    assert_eq!(state.ax(), 42);
    assert_eq!(state.sp(), 0x8000);
}

async fn bare_pending_hook() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    let input = Rc::new(Input::default());
    runner
        .install_hooks([Hook::new(0x200, {
            let cpu = runner.cpu_state();
            let source = input.clone();
            async move || {
                cpu.set_interrupts_enabled(true);
                cpu.set_ax(source.read().await);
                HookResult::Exit(ExecutionExit::Host(HostExit::Return))
            }
        })])
        .await
        .unwrap();
    pending(runner.run(10));
    irq.borrow_mut().raise();
    pending(runner.run(10));
    assert_eq!(irq.borrow().acknowledged, 0);
    input.send(42);
    assert_eq!(runner.run(10).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(runner.cpu_state().ax(), 42);
    assert_eq!(irq.borrow().acknowledged, 0);
}

async fn protected_software_interrupt() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    let state = runner.cpu_state();
    state.set_eflags(2);
    runner
        .memory()
        .view()
        .write(0x100, &[0xfb, 0xcd, 0x21, 0xeb, 0xfe])
        .unwrap();
    let input = Rc::new(Input::default());
    let starts = Rc::new(Cell::new(0));
    runner
        .install_hooks([real_mode_interrupt::hook(0x200, {
            let (cpu, memory, source, starts) = (
                state.clone(),
                runner.memory(),
                input.clone(),
                starts.clone(),
            );
            async move || {
                starts.set(starts.get() + 1);
                assert_eq!(cpu.instruction_count(), 2);
                assert!(!cpu.interrupts_enabled());
                assert_eq!(cpu.sp(), 0x7ffa);
                let mut frame = [0; 6];
                memory.view().read(0x7ffa, &mut frame).unwrap();
                assert_eq!(frame, [3, 1, 0, 0, 2, 2]);
                cpu.set_ax(real_mode_interrupt::with_interrupts_enabled(&cpu, source.read()).await);
                HookResult::Complete
            }
        })])
        .await
        .unwrap();
    irq.borrow_mut().raise();
    pending(runner.run(20));
    assert_eq!(starts.get(), 1);
    assert_eq!(
        irq.borrow().acknowledged,
        1,
        "INT retired the protected instruction before the host wait"
    );
    assert_eq!(runner.memory().view().read_u16(0x500).unwrap(), 1);
    assert_eq!(state.sp(), 0x7ffa);
    input.send(42);
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(state.ax(), 42);
    assert_eq!(state.sp(), 0x8000);
    assert_eq!(state.eip(), 0x103);
    assert!(state.interrupts_enabled());
}

test_backends!(
    service_resume_preserves_guest_effects_and_cs_alias,
    guest_effects
);
test_backends!(every_resume_entry_resumes_the_top_frame, top_frame_resume);
test_backends!(
    failed_irq_entry_retains_the_active_future_and_its_code_position,
    failed_entry
);
test_backends!(
    guest_isr_faults_leave_host_work_parked_until_repaired,
    isr_fault_and_repair
);
test_backends!(
    an_unrepresentable_original_position_leaves_the_frame_parked,
    resume_position_fault
);
test_backends!(
    bare_pending_hooks_leave_irqs_with_the_device,
    bare_pending_hook
);
test_backends!(
    software_int_after_sti_can_enter_an_interruptible_host_wait,
    protected_software_interrupt
);
