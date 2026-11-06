use super::support;
use relocade_compiler::Program;
use relocade_machine::{real_mode_interrupt, Cpu, DispatchEntry, Hook};
use relocade_x86::{
    abi, Compiler, CpuState, ExecutionExit, ExecutionExitValues, ExecutionProfile, HostExit,
    PhysicalMapping, PhysicalMemoryMap, Segments,
};
use std::{cell::Cell, rc::Rc};

async fn cpu() -> Cpu {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    support::create_cpu(config).await.unwrap()
}

fn resume(address: u32) -> Hook {
    Hook::block(address, |program| {
        program
            .function::<abi::ExecutionSignature>(|body, ()| {
                body.return_(ExecutionExitValues::host(HostExit::Resume))
            })
            .into()
    })
}

async fn replacement() {
    let mut runner = cpu().await;
    let state = runner.cpu_state();
    let owner = Rc::new(());
    runner
        .install(real_mode_interrupt::hook(0x200, state.clone(), {
            let owner = owner.clone();
            let exit = runner.exit_handle();
            async move || {
                let _borrow = &owner;
                exit.request();
            }
        }))
        .await
        .unwrap();
    // Overwriting the return entry leaves the host handler and Resume intact.
    runner.install([resume(0x202)]).await.unwrap();
    assert_eq!(Rc::strong_count(&owner), 2);
    state.set_eip(0x200);
    assert_eq!(runner.run().await, ExecutionExit::Host(HostExit::Return));
    for address in [0x201, 0x202] {
        state.set_eip(address);
        assert_eq!(runner.run().await, ExecutionExit::Host(HostExit::Resume));
    }

    // Replacing the host entry releases only that handler's captures.
    runner
        .install([Hook::block(0x200, |program| {
            program.import_function(abi::INTERPRET)
        })])
        .await
        .unwrap();
    assert_eq!(Rc::strong_count(&owner), 1);
    runner.memory().view().write(0x200, &[0xeb, 0xfe]).unwrap();
    state.set_eip(0x200);
    assert_eq!(
        crate::support::checkpoint(&mut runner).await,
        ExecutionExit::Interrupted
    );
    state.set_eip(0x201);
    assert_eq!(runner.run().await, ExecutionExit::Host(HostExit::Resume));

    // Reinstalling the adapter supplies all three positions again.
    runner
        .install(real_mode_interrupt::hook(0x200, state.clone(), {
            let exit = runner.exit_handle();
            async move || exit.request()
        }))
        .await
        .unwrap();
    state.set_eip(0x200);
    assert_eq!(runner.run().await, ExecutionExit::Host(HostExit::Return));
    // The return entry is back to the interpreter, which reads live guest bytes.
    runner.memory().view().write(0x202, &[0xeb, 0xfe]).unwrap();
    assert_eq!(
        crate::support::checkpoint(&mut runner).await,
        ExecutionExit::Interrupted
    );

    // Duplicate addresses use the final declaration, including across entry kinds.
    let discarded = Rc::new(());
    let released = Rc::downgrade(&discarded);
    runner
        .install([
            Hook::host(0x210, async move || {
                let _borrow = &discarded;
                panic!("superseded entry");
            }),
            resume(0x210),
        ])
        .await
        .unwrap();
    assert!(released.upgrade().is_none());
    state.set_eip(0x210);
    assert_eq!(runner.run().await, ExecutionExit::Host(HostExit::Resume));
}

async fn module_publication() {
    let mut runner = cpu().await;
    let state = runner.cpu_state();
    let owner = Rc::new(());
    runner
        .install([Hook::host(0x200, {
            let owner = owner.clone();
            let exit = runner.exit_handle();
            async move || {
                let _borrow = &owner;
                exit.request();
            }
        })])
        .await
        .unwrap();
    let mut program = Program::new();
    let entry = program.function::<abi::ExecutionSignature>(|body, ()| {
        body.return_(ExecutionExitValues::host(HostExit::Return))
    });
    program.export("exit", entry).unwrap();
    let bytes = program.compile().unwrap();
    assert!(runner
        .install_module(
            &bytes,
            &[
                DispatchEntry {
                    address: 0x200,
                    export: "exit"
                },
                DispatchEntry {
                    address: 0x220,
                    export: "missing"
                },
            ],
        )
        .await
        .is_err());
    assert!(runner
        .install([Hook::block(0x200, |program| {
            program
                .function::<fn() -> u32>(|body, ()| body.return_(0))
                .into()
        })])
        .await
        .is_err());
    assert_eq!(
        Rc::strong_count(&owner),
        2,
        "failed publication retains captures"
    );
    state.set_eip(0x200);
    assert_eq!(runner.run().await, ExecutionExit::Host(HostExit::Return));

    runner
        .install_module(
            &bytes,
            &[
                // A superseded export is not resolved, even if it is missing.
                DispatchEntry {
                    address: 0x200,
                    export: "missing",
                },
                DispatchEntry {
                    address: 0x200,
                    export: "exit",
                },
                DispatchEntry {
                    address: 0x210,
                    export: "exit",
                },
            ],
        )
        .await
        .unwrap();
    assert_eq!(Rc::strong_count(&owner), 1);
    // Exports from one module also remain independently replaceable.
    runner.install([resume(0x210)]).await.unwrap();
    state.set_eip(0x210);
    assert_eq!(runner.run().await, ExecutionExit::Host(HostExit::Resume));
    state.set_eip(0x200);
    assert_eq!(runner.run().await, ExecutionExit::Host(HostExit::Return));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn replacing_an_entry_preserves_its_neighbors() {
    pollster::block_on(replacement());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn replacing_an_entry_preserves_its_neighbors() {
    replacement().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn module_replacement_and_failed_publication_preserve_entry_ownership() {
    pollster::block_on(module_publication());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn module_replacement_and_failed_publication_preserve_entry_ownership() {
    module_publication().await;
}

async fn check_removed_entries() {
    let mut cpu = cpu().await;
    cpu.cpu_state().set_eip(0x100);
    cpu.memory()
        .view()
        .write(
            0x100,
            &[
                0xb8, 0x34, 0x12, // MOV AX,1234h.
                0xe9, 0xfa, 0, // JMP 0200h.
            ],
        )
        .unwrap();
    let capture = Rc::new(Cell::new(7));
    let weak = Rc::downgrade(&capture);
    let state = cpu.cpu_state();
    let exit = cpu.exit_handle();
    let finish = exit.clone();
    let observed = exit.clone();
    let before = observed.completed_exits();
    cpu.install([
        Hook::host(0x100, async move || {
            state.set_ax(capture.get());
            exit.request();
        }),
        Hook::host(0x200, async move || finish.request()),
    ])
    .await
    .unwrap();
    assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(cpu.cpu_state().ax(), 7);
    assert_eq!(observed.completed_exits(), before + 1);
    cpu.remove_entries([0x100, 0x999]);
    assert!(
        weak.upgrade().is_none(),
        "removal releases handler captures"
    );
    assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(cpu.cpu_state().ax(), 0x1234);
    assert_eq!(observed.completed_exits(), before + 2);
    let state = cpu.cpu_state();
    let exit = cpu.exit_handle();
    cpu.install([Hook::host(0x100, async move || {
        state.set_ax(9);
        exit.request();
    })])
    .await
    .unwrap();
    cpu.cpu_state().set_eip(0x100);
    assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(cpu.cpu_state().ax(), 9);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn removed_entries_execute_guest_bytes_and_can_be_reinstalled() {
    pollster::block_on(check_removed_entries());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn removed_entries_execute_guest_bytes_and_can_be_reinstalled() {
    check_removed_entries().await;
}
