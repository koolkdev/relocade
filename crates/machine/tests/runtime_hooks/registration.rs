//! Service entry/resume pairs, replacement and segment aliases.

use super::*;
use relocade_compiler::Program;
use relocade_machine::DispatchEntry;
use relocade_x86::{abi, ExecutionExitValues};

async fn check_hook_replacement() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x400,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let mut runtime = support::create_cpu((), config).await.unwrap();
    runtime.with(|call| call.memory.write(0x100, &[0x43, 0xe9, 0xfc, 2]).unwrap()); // INC BX; JMP 0400
    let first = Rc::new(Cell::new(0));
    let second = Rc::new(Cell::new(0));
    let released = Rc::downgrade(&first);
    let calls = Rc::clone(&second);
    runtime
        .install_hooks([
            Hook::new(0x400, 0x500, async move |mut call| {
                first.set(first.get() + 1);
                call.cpu.set_ax(first.get());
                call.cpu.set_eip(0x401);
                HookResult::Continue
            }),
            Hook::new(0x401, 0x501, async move |mut call| {
                calls.set(calls.get() + 1);
                call.cpu.set_eip(0x100);
                HookResult::Continue
            }),
        ])
        .await
        .unwrap();
    // Both hooks consume no instructions. Reentering Wasm retains one target.
    assert_eq!(runtime.run(3).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.with(|call| call.cpu.instruction_count()), 4);
    assert_eq!(runtime.with(|call| call.cpu.ax()), 2);
    assert_eq!(runtime.with(|call| call.cpu.bx()), 2);
    assert_eq!(second.get(), 2);
    runtime
        .install_hooks([Hook::new(0x400, 0x500, async |mut call| {
            call.cpu.set_ax(50);
            call.cpu.set_eip(0x401);
            HookResult::Continue
        })])
        .await
        .unwrap();
    assert!(released.upgrade().is_none());
    assert_eq!(runtime.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.with(|call| call.cpu.ax()), 50);
    assert_eq!(second.get(), 3);

    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::panic::{catch_unwind, AssertUnwindSafe};
        // An entry cannot overwrite another service's resume, and vice versa.
        for pairs in [
            vec![(0x500, 0x600)],
            vec![(0x600, 0x400)],
            vec![(0x600, 0x500)],
            vec![(0x600, 0x601), (0x601, 0x602)],
        ] {
            assert!(catch_unwind(AssertUnwindSafe(|| pollster::block_on(
                runtime.install_hooks(pairs.into_iter().map(|(entry, resume)| Hook::new(
                    entry,
                    resume,
                    async |_| HookResult::Continue
                )))
            )))
            .is_err());
        }
    }

    let mut program = Program::new();
    let entry = program.function::<abi::ExecutionSignature>(|body, ()| {
        body.return_(ExecutionExitValues::host(HostExit::Return))
    });
    program.export("entry", entry).unwrap();
    let bytes = program.compile().unwrap();
    // An unresolved later export preserves both the hook and its captured state.
    assert!(runtime
        .install_module(
            &bytes,
            &[
                DispatchEntry {
                    address: 0x401,
                    export: "entry"
                },
                DispatchEntry {
                    address: 0x402,
                    export: "missing"
                },
            ]
        )
        .await
        .is_err());
    runtime.with(|call| call.cpu.set_eip(0x401));
    assert_eq!(runtime.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(second.get(), 4);
    assert_eq!(Rc::strong_count(&second), 2);
    runtime.with(|call| call.cpu.set_eip(0x501));
    assert_eq!(
        runtime.run(1).await,
        ExecutionExit::Host(HostExit::Resume { address: 0x401 }),
        "failed replacement retains both labels"
    );
    runtime
        .install_module(
            &bytes,
            &[DispatchEntry {
                address: 0x401,
                export: "entry",
            }],
        )
        .await
        .unwrap();
    assert_eq!(Rc::strong_count(&second), 1);
    runtime.with(|call| call.cpu.set_eip(0x401));
    assert_eq!(runtime.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(second.get(), 4);
    runtime.with(|call| {
        call.memory.write(0x501, &[0xeb, 0xfe]).unwrap();
        call.cpu.set_eip(0x501);
    });
    assert_eq!(
        runtime.run(1).await,
        ExecutionExit::BudgetExhausted,
        "replacing the service removes its resume label too"
    );
    runtime
        .install_hooks([Hook::new(0x400, 0x600, async |_| {
            HookResult::Exit(ExecutionExit::Host(HostExit::Return))
        })])
        .await
        .unwrap();
    runtime.with(|call| {
        call.memory.write(0x500, &[0xeb, 0xfe]).unwrap();
        call.cpu.set_eip(0x500);
    });
    assert_eq!(
        runtime.run(1).await,
        ExecutionExit::BudgetExhausted,
        "moving a service's resume removes the old label"
    );
    runtime.with(|call| call.cpu.set_eip(0x400));
    assert_eq!(runtime.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(runtime.with(|call| call.cpu.eip()), 0x600);
}

async fn check_resume_alias_limit() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0xffff,
            segments: Segments::real_mode(),
            ..Default::default()
        },
        0x20000,
        PhysicalMemoryMap::new([(0..=0x1ffff, PhysicalMapping::Ram { backing_offset: 0 })])
            .unwrap(),
    )
    .unwrap();
    let mut cpu = support::create_cpu((), config).await.unwrap();
    cpu.install_hooks([Hook::new(0xffff, 0x10000, async |call| {
        assert_eq!(call.cpu.segment(Segment::Cs).selector, 1);
        assert_eq!(call.cpu.eip(), 0xfff0);
        HookResult::Exit(ExecutionExit::Host(HostExit::Return))
    })])
    .await
    .unwrap();
    let before = cpu.with(|call| call.cpu.snapshot());
    assert_eq!(
        cpu.run(1).await,
        ExecutionExit::Exception(relocade_x86::Exception::GeneralProtection { error_code: 0 })
    );
    assert_eq!(cpu.with(|call| call.cpu.snapshot()), before);
    cpu.with(|call| {
        call.cpu.set_segment(
            Segment::Cs,
            relocade_x86::SegmentRegister::real_mode(Segment::Cs, 1),
        );
        call.cpu.set_eip(0xffef);
    });
    assert_eq!(cpu.run(1).await, ExecutionExit::Host(HostExit::Return));
    cpu.install_hooks([real_mode_interrupt::hook(0x1fffe, async |mut call| {
        real_mode_interrupt::return_from_interrupt(&mut call.cpu)
    })])
    .await
    .unwrap();
    cpu.with(|call| {
        call.cpu.set_segment(
            Segment::Cs,
            relocade_x86::SegmentRegister::real_mode(Segment::Cs, 0x1000),
        );
        call.cpu.set_eip(0xfffe);
    });
    assert_eq!(
        cpu.run(1).await,
        ExecutionExit::Exception(relocade_x86::Exception::GeneralProtection { error_code: 0 })
    );
    assert_eq!(
        cpu.with(|call| call.cpu.eip()),
        0x10000,
        "completion must not wrap the finish label into unrelated code"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn service_resume_preserves_cs_aliases_and_checks_their_limit() {
    pollster::block_on(check_resume_alias_limit());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn service_resume_preserves_cs_aliases_and_checks_their_limit() {
    check_resume_alias_limit().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn local_hooks_preserve_budgets_and_release_captures_when_replaced() {
    pollster::block_on(check_hook_replacement());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn local_hooks_preserve_budgets_and_release_captures_when_replaced() {
    check_hook_replacement().await;
}
