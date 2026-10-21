use relocade_machine::MachineConfig;
use relocade_machine::{real_mode_interrupt, Cpu, Hook, RuntimeState};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segment, Segments,
};
use std::{cell::Cell, ops::ControlFlow, rc::Rc};

use relocade_wasm_runtime::Backend;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_interrupt_hooks() {
    let config = MachineConfig::new(
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
    // Non-Send state belongs to the backend, while hooks may capture immutable metadata.
    let calls = Rc::new(Cell::new(0));
    let backend = Backend::with_state(RuntimeState::new(calls)).unwrap();
    let mut runtime = Cpu::new(backend, config).await.unwrap();
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
    let increment = 1;
    runtime
        .install_hooks([
            Hook::<Rc<Cell<u32>>>::new(0x10020, move |call| {
                call.state().set(call.state().get() + increment);
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
            Hook::new(0x400, |_| ControlFlow::Break(ExecutionExit::HostReturn)),
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

    runtime.context().cpu().set_eip(0x400);
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::HostReturn);
    assert_eq!(runtime.context().cpu().instruction_count(), 14);
    assert_eq!(runtime.context().cpu().eip(), 0x400);
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
