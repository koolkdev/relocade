use super::{cpu, pending, Input, Irq, Operation};
use relocade_machine::{Hook, HookResult};
use relocade_x86::{ExecutionExit, HostExit, Segment};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

async fn guest_effects() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut cpu = cpu(&irq, 0x300).await;
    cpu.memory()
        .view()
        .write(
            0x300,
            &[
                0xb8, 0x20, 0, 0x8e, 0xd0, 0xbc, 0, 0x60, 0xeb,
                0, // SS=20, SP=6000; JMP next.
                0xb8, 0x30, 0, 0x8e, 0xd8, 0xbb, 0x67, 0x45, // DS=30, BX=4567.
                0x2e, 0xc7, 0x06, 0, 5, 0xfe, 0xca, // MOV word CS:[0500],CAFE.
                0x68, 0x46, 6, 0x6a, 0x10, 0x68, 1, 1, 0xcf, // New FLAGS/CS/IP; IRET.
            ],
        )
        .unwrap();
    let input = Rc::new(Input::default());
    let source = input.clone();
    cpu.install_hooks([Hook::new(0x200, 0x201, async move |mut call| {
        call.cpu.set_interrupts_enabled(true);
        assert_eq!(source.read().await, 7);
        assert_eq!(call.cpu.segment(Segment::Cs).selector, 0x10);
        assert_eq!(call.cpu.eip(), 0x101); // Preserve the guest's alias of the resume label.
        assert_eq!(call.cpu.ax(), 0x30);
        assert_eq!(call.cpu.bx(), 0x4567);
        assert_eq!(call.cpu.segment(Segment::Ds).selector, 0x30);
        assert_eq!(call.cpu.segment(Segment::Ss).selector, 0x20);
        assert_eq!(call.cpu.sp(), 0x6000);
        assert_eq!(call.cpu.eflags(), 0x646);
        assert_eq!(call.memory.view().read_u16(0x500).unwrap(), 0xcafe);
        HookResult::Exit(ExecutionExit::Host(HostExit::Return))
    })])
    .await
    .unwrap();
    pending(cpu.run(10));
    irq.borrow_mut().raise();
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
    let mut frame = [0; 6];
    cpu.memory().view().read(0x7ff4, &mut frame).unwrap();
    assert_eq!(
        frame,
        [1, 2, 0, 0, 0x43, 2],
        "IRQ return names the resume entry"
    );
    input.send(7);
    assert_eq!(input.consumed.get(), 0);
    assert_eq!(cpu.run(100).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(input.consumed.get(), 1);
}

async fn redirected_hook() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut cpu = cpu(&irq, 0x300).await;
    // Discard the IRQ frame and jump to the original hook with exactly its old SS:SP.
    cpu.memory()
        .view()
        .write(0x300, &[0x83, 0xc4, 6, 0xe9, 0xfa, 0xfe])
        .unwrap();
    let input = Rc::new(Input::default());
    let starts = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    let (source, started, dropped) = (input.clone(), starts.clone(), drops.clone());
    cpu.install_hooks([Hook::new(0x200, 0x201, async move |call| {
        let index = started.get();
        started.set(index + 1);
        if index == 0 {
            let _operation = Operation(dropped.clone());
            call.with_interrupts_enabled(source.read()).await;
            panic!("redirecting to a service must not resume its parked invocation");
        }
        HookResult::Exit(ExecutionExit::Host(HostExit::Return))
    })])
    .await
    .unwrap();
    pending(cpu.run(10));
    irq.borrow_mut().raise();
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
    #[cfg(not(target_arch = "wasm32"))]
    {
        use relocade_machine::DispatchEntry;
        use std::panic::{catch_unwind, AssertUnwindSafe};
        let mut program = relocade_compiler::Program::new();
        let entry = program.function::<relocade_x86::abi::ExecutionSignature>(|body, ()| {
            body.return_(relocade_x86::ExecutionExitValues::host(HostExit::Return))
        });
        program.export("entry", entry).unwrap();
        let bytes = program.compile().unwrap();
        assert!(
            catch_unwind(AssertUnwindSafe(|| pollster::block_on(cpu.install_hooks(
                [Hook::new(0x200, 0x202, async |_| HookResult::Continue)]
            ))))
            .is_err(),
            "a parked service keeps its resume label"
        );
        for address in [0x200, 0x201] {
            assert!(
                catch_unwind(AssertUnwindSafe(|| pollster::block_on(cpu.install_module(
                    &bytes,
                    &[DispatchEntry {
                        address,
                        export: "entry"
                    }]
                ))))
                .is_err(),
                "a parked service keeps both labels installed"
            );
        }
    }
    input.send(7);
    assert_eq!(cpu.run(10).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(starts.get(), 2);
    assert_eq!(input.consumed.get(), 0);
    assert_eq!(drops.get(), 1);
    assert!(!cpu.with(|call| call.cpu.interrupts_enabled()));
}

async fn abandon_redirected_frame() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut cpu = cpu(&irq, 0x300).await;
    let drops = Rc::new(Cell::new(0));
    let dropped = drops.clone();
    cpu.install_hooks([Hook::new(0x200, 0x201, async move |mut call| {
        let _operation = Operation(dropped.clone());
        call.cpu.set_interrupts_enabled(true);
        call.with_interrupts_enabled(std::future::pending::<()>())
            .await;
        unreachable!()
    })])
    .await
    .unwrap();
    pending(cpu.run(10));
    irq.borrow_mut().raise();
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
    cpu.with(|call| {
        call.cpu.set_eip(0x400);
        call.cpu.set_eflags(0x43);
        call.memory.write(0x400, &[0xeb, 0xfe]).unwrap();
    });
    cpu.abandon_suspended_calls();
    assert_eq!(drops.get(), 1);
    assert_eq!(cpu.with(|call| call.cpu.eflags()), 0x43);
    assert_eq!(cpu.run(2).await, ExecutionExit::BudgetExhausted);
}

async fn ready_await() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut cpu = cpu(&irq, 0x300).await;
    let device = irq.clone();
    cpu.install_hooks([Hook::new(0x200, 0x201, async move |mut call| {
        call.cpu.set_interrupts_enabled(true);
        device.borrow_mut().raise();
        let value = std::future::ready(7).await;
        assert_eq!(device.borrow().acknowledged, 0);
        call.cpu.set_ax(value);
        HookResult::Exit(ExecutionExit::Host(HostExit::Return))
    })])
    .await
    .unwrap();
    assert_eq!(cpu.run(10).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(cpu.with(|call| call.cpu.ax()), 7);
    assert_eq!(irq.borrow().acknowledged, 0);
}

async fn unmatched_resume() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut cpu = cpu(&irq, 0x300).await;
    let input = Rc::new(Input::default());
    let drops = Rc::new(Cell::new(0));
    let (source, dropped) = (input.clone(), drops.clone());
    cpu.install_hooks([
        Hook::new(0x200, 0x201, async move |call| {
            let _operation = Operation(dropped.clone());
            assert_eq!(call.cpu.eip(), 0x201);
            assert_eq!(call.with_interrupts_enabled(source.read()).await, 7);
            HookResult::Exit(ExecutionExit::Host(HostExit::Return))
        }),
        Hook::new(0x400, 0x401, async |_| {
            panic!("resume never enters a service")
        }),
    ])
    .await
    .unwrap();
    pending(cpu.run(10));
    irq.borrow_mut().raise();
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
    input.send(7);
    cpu.with(|call| call.cpu.set_eip(0x401));
    assert_eq!(
        cpu.run(10).await,
        ExecutionExit::Host(HostExit::Resume { address: 0x400 })
    );
    assert_eq!(input.consumed.get(), 0);
    assert_eq!(drops.get(), 0);
    // Finish the original ISR. Its IRET still resumes the retained top frame.
    cpu.with(|call| call.cpu.set_eip(0x306));
    assert_eq!(cpu.run(10).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(input.consumed.get(), 1);
    assert_eq!(drops.get(), 1);
    cpu.with(|call| call.cpu.set_eip(0x201));
    let before = cpu.with(|call| call.cpu.snapshot());
    assert_eq!(
        cpu.run(10).await,
        ExecutionExit::Host(HostExit::Resume { address: 0x200 })
    );
    assert_eq!(cpu.with(|call| call.cpu.snapshot()), before);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn service_resume_preserves_all_guest_effects_including_cs_alias() {
    pollster::block_on(guest_effects());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn service_resume_preserves_all_guest_effects_including_cs_alias() {
    guest_effects().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn same_hook_and_stack_start_a_new_invocation_until_explicit_resume() {
    pollster::block_on(redirected_hook());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn same_hook_and_stack_start_a_new_invocation_until_explicit_resume() {
    redirected_hook().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn explicit_abandonment_releases_redirected_work_without_restoring_if() {
    pollster::block_on(abandon_redirected_frame());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn explicit_abandonment_releases_redirected_work_without_restoring_if() {
    abandon_redirected_frame().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ready_awaits_continue_synchronously_with_a_pending_irq() {
    pollster::block_on(ready_await());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn ready_awaits_continue_synchronously_with_a_pending_irq() {
    ready_await().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unmatched_resume_preserves_suspended_work_and_guest_state() {
    pollster::block_on(unmatched_resume());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn unmatched_resume_preserves_suspended_work_and_guest_state() {
    unmatched_resume().await;
}
