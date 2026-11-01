use super::*;
use relocade_compiler::Program;
use relocade_machine::DispatchEntry;
use relocade_x86::{abi, ExecutionExitValues};

async fn check_completion_aliases() {
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
    let mut cpu = support::create_cpu(config).await.unwrap();
    // The resume position is outside this CS. Ready exit needs neither resume
    // preparation nor a suspended execution frame.
    cpu.install_hooks([Hook::new(0xffff, {
        let state = cpu.cpu_state();
        let stop = cpu.stop_handle();
        async move || {
            assert_eq!(state.eip(), 0xffff);
            stop.stop();
        }
    })
    .with_continuations(0x10001, 0x10000)])
        .await
        .unwrap();
    assert_eq!(cpu.run(1).await, ExecutionExit::Host(HostExit::Return));
    cpu.install_hooks([
        Hook::new(0xffff, async || {}).with_continuations(0x10001, 0x10000),
        Hook::new(0x10000, async || {
            panic!("invalid completion must not enter another service")
        }),
    ])
    .await
    .unwrap();
    assert_eq!(
        cpu.run(1).await,
        ExecutionExit::Exception(relocade_x86::Exception::GeneralProtection { error_code: 0 })
    );
    assert_eq!(
        cpu.context().cpu.eip(),
        0x10000,
        "completion must not wrap to zero"
    );
    // Replace the completion hook before testing a valid CS alias.
    let mut program = Program::new();
    let exit = program.function::<abi::ExecutionSignature>(|body, ()| {
        body.return_(ExecutionExitValues::budget_exhausted())
    });
    program.export("exit", exit).unwrap();
    cpu.install_module(
        &program.compile().unwrap(),
        &[DispatchEntry {
            address: 0x10000,
            export: "exit",
        }],
    )
    .await
    .unwrap();
    {
        let mut context = cpu.context();
        context.cpu.set_segment(
            Segment::Cs,
            relocade_x86::SegmentRegister::real_mode(Segment::Cs, 1),
        );
        context.cpu.set_eip(0xffef);
        context.memory.write(0x10000, &[0xeb, 0xfe]).unwrap();
    }
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(cpu.context().cpu.segment(Segment::Cs).selector, 1);
    assert_eq!(cpu.context().cpu.eip(), 0xfff0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn completion_uses_the_guest_cs_alias_without_wrapping_or_preparing_resume() {
    pollster::block_on(check_completion_aliases());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn completion_uses_the_guest_cs_alias_without_wrapping_or_preparing_resume() {
    check_completion_aliases().await;
}

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
    let mut runtime = support::create_cpu(config).await.unwrap();
    runtime
        .context()
        .memory
        .write(0x100, &[0x43, 0xe9, 0xfc, 2])
        .unwrap(); // INC BX; JMP 0400
    let first = Rc::new(Cell::new(0));
    let second = Rc::new(Cell::new(0));
    let released = Rc::downgrade(&first);
    let calls = Rc::clone(&second);
    runtime
        .install_hooks([
            Hook::new(0x400, {
                let cpu = runtime.cpu_state();
                async move || {
                    first.set(first.get() + 1);
                    cpu.set_ax(first.get());
                }
            })
            .with_continuations(0x500, 0x401),
            Hook::new(0x401, {
                async move || {
                    calls.set(calls.get() + 1);
                }
            })
            .with_continuations(0x501, 0x100),
        ])
        .await
        .unwrap();
    // Both hooks consume no instructions. Reentering Wasm retains one target.
    assert_eq!(runtime.run(3).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu.instruction_count(), 4);
    assert_eq!(runtime.context().cpu.ax(), 2);
    assert_eq!(runtime.context().cpu.bx(), 2);
    assert_eq!(second.get(), 2);
    runtime
        .install_hooks([Hook::new(0x400, {
            let cpu = runtime.cpu_state();
            async move || {
                cpu.set_ax(50);
            }
        })
        .with_continuations(0x600, 0x401)])
        .await
        .unwrap();
    assert!(released.upgrade().is_none());
    assert_eq!(runtime.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu.ax(), 50);
    assert_eq!(second.get(), 3);

    // Moving a service's resume removes its old dispatch entry.
    runtime
        .context()
        .memory
        .write(0x500, &[0xeb, 0xfe])
        .unwrap();
    runtime.context().cpu.set_eip(0x500);
    assert_eq!(runtime.run(1).await, ExecutionExit::BudgetExhausted);
    for (entry, resume) in [(0x501, 0x700), (0x700, 0x401), (0x700, 0x501)] {
        assert!(runtime
            .install_hooks([Hook::new(entry, async || {}).with_continuations(resume, 0x100)])
            .await
            .is_err());
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
    runtime.context().cpu.set_eip(0x401);
    assert_eq!(runtime.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(second.get(), 4);
    assert_eq!(Rc::strong_count(&second), 2);
    runtime.context().cpu.set_eip(0x501);
    assert!(matches!(
        runtime.run(1).await,
        ExecutionExit::Host(HostExit::Resume)
    ));
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
    runtime.context().cpu.set_eip(0x401);
    assert_eq!(runtime.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(second.get(), 4);
    runtime
        .context()
        .memory
        .write(0x501, &[0xeb, 0xfe])
        .unwrap();
    runtime.context().cpu.set_eip(0x501);
    assert_eq!(runtime.run(1).await, ExecutionExit::BudgetExhausted);
    // Reinstalling after a module replacement routes to the new service.
    // Duplicate addresses in one batch publish the last registration and release
    // the unused capture, without disturbing any other service.
    for address in [0x401, 0x700] {
        let unused = Rc::new(());
        let released = Rc::downgrade(&unused);
        let unused_cpu = runtime.cpu_state();
        let unused_stop = runtime.stop_handle();
        let stop = runtime.stop_handle();
        let cpu = runtime.cpu_state();
        runtime
            .install_hooks([
                Hook::new(address, async move || {
                    let _borrow = &unused;
                    unused_cpu.set_ax(100);
                    unused_stop.stop();
                }),
                Hook::new(address, async move || {
                    cpu.set_ax(75);
                    stop.stop();
                }),
            ])
            .await
            .unwrap();
        assert!(released.upgrade().is_none());
        runtime.context().cpu.set_eip(address);
        assert_eq!(runtime.run(1).await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(runtime.context().cpu.ax(), 75);
        assert_eq!(second.get(), 4);
    }
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
