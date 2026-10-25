mod support;

use relocade_compiler::Program;
use relocade_machine::{real_mode_interrupt, DispatchEntry, Hook, HookResult, RuntimeState};
use relocade_x86::{
    abi, Compiler, CpuState, ExecutionExit, ExecutionExitValues, ExecutionProfile, HostExit,
    PhysicalMapping, PhysicalMemoryMap, Segment, Segments,
};
use std::{cell::Cell, rc::Rc};

use relocade_wasm_runtime::Backend;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_interrupt_hooks() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x20000,
        PhysicalMemoryMap::new([(0..=0x1ffff, PhysicalMapping::Ram { backing_offset: 0 })])
            .unwrap(),
    )
    .unwrap();
    // Both environment state and handler captures may be local to this thread.
    let calls = Rc::new(Cell::new(0));
    let backend = Backend::with_state(RuntimeState::new(calls)).unwrap();
    let mut runtime = support::create_cpu(backend, config).await.unwrap();
    runtime.context().cpu().set_sp(0xff00);
    runtime.context().cpu().set_eflags(0x202);
    runtime
        .context()
        .memory()
        .write(
            0x100,
            &[
                0xb8, 0xff, 0xff, // MOV AX,FFFF
                0xc7, 0x06, 0, 2, 10, 0, // MOV word [0200],10
                0x83, 0xc0, 2, // ADD AX,2 (sets carry)
                0xcd, 0x21, // INT 21h
                0xa3, 2, 2, // MOV [0202],AX
                0xeb, 0xed, // JMP 0100
            ],
        )
        .unwrap();
    runtime
        .context()
        .memory()
        .write(0x84, &[0x20, 0, 0, 0x10])
        .unwrap();
    runtime
        .context()
        .memory()
        .write(0x10020, &real_mode_interrupt::HOOK_STUB)
        .unwrap();
    let increment = Rc::new(Cell::new(1));
    runtime
        .install_hooks([
            Hook::<Rc<Cell<u32>>>::new(0x10020, move |call| {
                call.state().set(call.state().get() + increment.get());
                assert_eq!(call.cpu().segment(Segment::Cs).selector, 0x1000);
                assert_eq!(call.cpu().eip(), 0x20);
                assert_eq!(call.cpu().sp(), 0xfefa);
                assert_eq!(call.cpu().ax(), 1);
                assert!(call.cpu().carry());
                assert_eq!(call.cpu().eflags() & 0x200, 0);
                let mut word = [0; 2];
                call.memory().read(0x200, &mut word).unwrap();
                call.cpu().set_ax(1 + u16::from_le_bytes(word));
                let mut frame = [0; 6];
                call.memory().read(0xfefa, &mut frame).unwrap();
                assert_eq!(&frame[..4], &[0x0e, 1, 0, 0]);
                assert_eq!(u16::from_le_bytes([frame[4], frame[5]]) & 0x201, 0x201);
                real_mode_interrupt::set_carry(call, false);
                real_mode_interrupt::return_from_interrupt(call)
            }),
            Hook::new(0x400, |_| {
                HookResult::Exit(ExecutionExit::Host(HostExit::Return))
            }),
            Hook::new(0x401, |_| {
                HookResult::Exit(ExecutionExit::Exception(
                    relocade_x86::Exception::PageFault {
                        linear_address: 0xfedc_ba98,
                        error_code: 0xdead_beef,
                    },
                ))
            }),
            Hook::new(0x402, |_| {
                HookResult::Exit(ExecutionExit::UnsupportedInstruction {
                    eip: 0x89ab_cdef,
                    opcode: 0xf3,
                })
            }),
        ])
        .await
        .unwrap();
    assert_eq!(runtime.run(5).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.state().get(), 1);
    assert_eq!(runtime.context().cpu().ax(), 11);
    assert_eq!(runtime.context().cpu().segment(Segment::Cs).selector, 0);
    assert_eq!(runtime.context().cpu().eip(), 0x10e);
    assert_eq!(runtime.context().cpu().sp(), 0xff00);
    assert_eq!(runtime.context().cpu().eflags() & 0x201, 0x200);
    assert_eq!(runtime.run(2).unwrap(), ExecutionExit::BudgetExhausted);
    let mut result = [0; 2];
    runtime.context().memory().read(0x202, &mut result).unwrap();
    assert_eq!(u16::from_le_bytes(result), 11);

    // Replace the IVT destination with guest code that chains to the original hook.
    runtime
        .context()
        .memory()
        .write(0x84, &[0, 5, 0, 0])
        .unwrap();
    runtime
        .context()
        .memory()
        .write(0x500, &[0x43, 0xea, 0x20, 0, 0, 0x10])
        .unwrap();
    assert_eq!(runtime.run(7).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.state().get(), 2);
    assert_eq!(runtime.context().cpu().bx(), 1);
    assert_eq!(runtime.context().cpu().eip(), 0x10e);
    assert_eq!(runtime.context().cpu().sp(), 0xff00);
    assert_eq!(runtime.context().cpu().instruction_count(), 14);

    for (address, expected) in [
        (0x400, ExecutionExit::Host(HostExit::Return)),
        (
            0x401,
            ExecutionExit::Exception(relocade_x86::Exception::PageFault {
                linear_address: 0xfedc_ba98,
                error_code: 0xdead_beef,
            }),
        ),
        (
            0x402,
            ExecutionExit::UnsupportedInstruction {
                eip: 0x89ab_cdef,
                opcode: 0xf3,
            },
        ),
    ] {
        runtime.context().cpu().set_eip(address);
        assert_eq!(runtime.run(1).unwrap(), expected);
        assert_eq!(runtime.context().cpu().instruction_count(), 14);
        assert_eq!(runtime.context().cpu().eip(), address);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn interrupt_hooks_return_through_guest_frames_and_allow_chaining() {
    pollster::block_on(check_interrupt_hooks());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn interrupt_hooks_return_through_guest_frames_and_allow_chaining() {
    check_interrupt_hooks().await;
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
    let backend = Backend::with_state(RuntimeState::new(())).unwrap();
    let mut runtime = support::create_cpu(backend, config).await.unwrap();
    runtime
        .context()
        .memory()
        .write(0x100, &[0x43, 0xe9, 0xfc, 2])
        .unwrap(); // INC BX; JMP 0400
    let first = Rc::new(Cell::new(0));
    let second = Rc::new(Cell::new(0));
    let released = Rc::downgrade(&first);
    let calls = Rc::clone(&second);
    runtime
        .install_hooks([
            Hook::new(0x400, move |call| {
                first.set(first.get() + 1);
                call.cpu().set_ax(first.get());
                call.cpu().set_eip(0x401);
                HookResult::Continue
            }),
            Hook::new(0x401, move |call| {
                calls.set(calls.get() + 1);
                call.cpu().set_eip(0x100);
                HookResult::Continue
            }),
        ])
        .await
        .unwrap();
    // Both hooks consume no instructions. Reentering Wasm retains one target.
    assert_eq!(runtime.run(3).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu().instruction_count(), 4);
    assert_eq!(runtime.context().cpu().ax(), 2);
    assert_eq!(runtime.context().cpu().bx(), 2);
    assert_eq!(second.get(), 2);
    runtime
        .install_hooks([Hook::new(0x400, |call| {
            call.cpu().set_ax(50);
            call.cpu().set_eip(0x401);
            HookResult::Continue
        })])
        .await
        .unwrap();
    assert!(released.upgrade().is_none());
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu().ax(), 50);
    assert_eq!(second.get(), 3);

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
    runtime.context().cpu().set_eip(0x401);
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(second.get(), 4);
    assert_eq!(Rc::strong_count(&second), 2);
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
    runtime.context().cpu().set_eip(0x401);
    assert_eq!(
        runtime.run(1).unwrap(),
        ExecutionExit::Host(HostExit::Return)
    );
    assert_eq!(second.get(), 4);
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
