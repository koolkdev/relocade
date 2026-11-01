use super::{cpu, pending, Input, Irq, Operation};
use relocade_machine::{real_mode_interrupt, Entry, Hook};
use relocade_x86::{abi, Exception, ExecutionExit, ExecutionExitValues, HostExit, Segment};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

async fn guest_effects() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    let state = runner.cpu_state();
    let memory = runner.memory();
    // The ISR returns through a CS alias that can represent the resume and IRET
    // positions, but not the original hook entry. No saved position is needed.
    memory.view().write(0x84, &[0x0f, 2, 0, 0]).unwrap();
    memory
        .view()
        .write(0x20f, &real_mode_interrupt::HOOK_STUB)
        .unwrap();
    memory
        .view()
        .write(
            0x300,
            &[
                0xb8, 0x20, 0, 0x8e, 0xd0, 0xbc, 0, 0x60, 0xeb,
                0, // SS=20, SP=6000; JMP next.
                0xb8, 0x30, 0, 0x8e, 0xd8, 0xbb, 0x67, 0x45, // DS=30, BX=4567.
                0x2e, 0xc7, 0x06, 0, 5, 0xfe, 0xca, // MOV word CS:[0500],CAFE.
                0x68, 0x46, 6, 0x6a, 0x21, 0x6a, 0,
                0xcf, // New FLAGS/CS/IP; IRET to 0021:0000.
            ],
        )
        .unwrap();
    let input = Rc::new(Input::default());
    runner
        .install(real_mode_interrupt::hook(0x20f, runner.cpu_state(), {
            let cpu = state.clone();
            let memory = memory.clone();
            let source = input.clone();
            let exit = runner.exit_handle();
            async move || {
                cpu.set_interrupts_enabled(true);
                assert_eq!(source.read().await, 7);
                assert_eq!(cpu.segment(Segment::Cs).selector, 0x21);
                assert_eq!(cpu.eip(), 0, "keep the live resume position and CS alias");
                assert_eq!(cpu.ax(), 0x30);
                assert_eq!(cpu.bx(), 0x4567);
                assert_eq!(cpu.segment(Segment::Ds).selector, 0x30);
                assert_eq!(cpu.segment(Segment::Ss).selector, 0x20);
                assert_eq!(cpu.sp(), 0x6000);
                assert_eq!(cpu.eflags(), 0x646);
                assert_eq!(memory.view().read_u16(0x500).unwrap(), 0xcafe);
                exit.request();
            }
        }))
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
        .flat_map(|(index, entry)| {
            let (cpu, source, starts, finished) = (
                state.clone(),
                inputs[index].clone(),
                starts.clone(),
                finished.clone(),
            );
            real_mode_interrupt::hook(entry, cpu.clone(), async move || {
                starts.set(starts.get() + 1);
                let value = real_mode_interrupt::with_interrupts_enabled(&cpu, source.read()).await;
                assert_eq!(cpu.eip(), 0x201, "keep the guest's chosen resume label");
                finished.borrow_mut().push((index, value));
            })
        });
    runner.install(hooks).await.unwrap();
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
        .write_u16(u32::from(state.sp()), 0x201)
        .unwrap();
    assert_eq!(runner.run(10).await, ExecutionExit::BudgetExhausted);
    assert_eq!(*finished.borrow(), [(1, 20), (0, 10)]);
    assert_eq!(inputs[0].consumed.get(), 1);
    assert_eq!(inputs[1].consumed.get(), 1);
    assert_eq!(state.sp(), 0x8000);
    state.set_eip(0x201); // No suspended frame remains.
    let before = state.snapshot();
    assert!(matches!(
        runner.run(10).await,
        ExecutionExit::Host(HostExit::Resume)
    ));
    assert_eq!(state.snapshot(), before);
    assert_eq!(starts.get(), 2, "resume entries never invoke a handler");
}

async fn failed_entry() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    let input = Rc::new(Input::default());
    let state = runner.cpu_state();
    let drops = Rc::new(Cell::new(0));
    runner
        .install(real_mode_interrupt::hook(0x200, runner.cpu_state(), {
            let (cpu, source, dropped) = (state.clone(), input.clone(), drops.clone());
            async move || {
                let _operation = Operation(dropped.clone());
                cpu.set_interrupts_enabled(true);
                cpu.set_ax(source.read().await);
            }
        }))
        .await
        .unwrap();
    pending(runner.run(10));
    state.set_sp(4);
    let before = state.snapshot();
    assert_eq!(before.eip, 0x201);
    irq.borrow_mut().raise();
    assert_eq!(
        runner.run(10).await,
        ExecutionExit::Exception(Exception::StackFault { error_code: 0 })
    );
    assert_eq!(
        state.snapshot(),
        before,
        "entry fault preserves the live continuation"
    );
    assert_eq!(irq.borrow().acknowledged, 1);
    assert_eq!(drops.get(), 0);
    state.set_sp(0x7ffa);
    input.send(42);
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(state.ax(), 42);
    assert_eq!(state.sp(), 0x8000);
    assert_eq!(drops.get(), 1);
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
        .install(real_mode_interrupt::hook(0x200, runner.cpu_state(), {
            let (cpu, source) = (state.clone(), input.clone());
            async move || {
                cpu.set_ax(real_mode_interrupt::with_interrupts_enabled(&cpu, source.read()).await);
            }
        }))
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

async fn raw_hook_continuation() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    let input = Rc::new(Input::default());
    let state = runner.cpu_state();
    runner
        .install([
            Hook::new(0x200, {
                let cpu = state.clone();
                let source = input.clone();
                let exit = runner.exit_handle();
                async move || {
                    cpu.set_eip(0x600);
                    cpu.set_interrupts_enabled(true);
                    cpu.set_ax(source.read().await);
                    assert_eq!(cpu.eip(), 0x600);
                    exit.request();
                }
            })
            .into(),
            Entry::block(0x600, |program| {
                program
                    .function::<abi::ExecutionSignature>(|body, ()| {
                        body.return_(ExecutionExitValues::host(HostExit::Resume))
                    })
                    .into()
            }),
        ])
        .await
        .unwrap();
    pending(runner.run(10));
    irq.borrow_mut().raise();
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(irq.borrow().acknowledged, 1);
    assert_eq!(runner.memory().view().read_u16(0x7ff4).unwrap(), 0x600);
    input.send(42);
    assert_eq!(input.consumed.get(), 0);
    assert_eq!(runner.run(10).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(state.ax(), 42);
    assert_eq!(state.eip(), 0x600);
    assert_eq!(input.consumed.get(), 1);
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
        .install(real_mode_interrupt::hook(0x200, runner.cpu_state(), {
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
            }
        }))
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
    failed_irq_entry_preserves_the_active_future_and_live_continuation,
    failed_entry
);
test_backends!(
    guest_isr_faults_leave_host_work_parked_until_repaired,
    isr_fault_and_repair
);
test_backends!(
    raw_hooks_supply_their_live_irq_return_continuation,
    raw_hook_continuation
);
test_backends!(
    software_int_after_sti_can_enter_an_interruptible_host_wait,
    protected_software_interrupt
);
