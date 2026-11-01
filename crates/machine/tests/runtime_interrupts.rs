#[path = "support/interrupts.rs"]
mod interrupts;
mod support;

use interrupts::Irq;
use relocade_machine::{Cpu, DeviceConnection, Hook, HookResult, IoBus, Memory};
use relocade_x86::{
    Compiler, CpuState, Exception, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap, Segment, SegmentRegister, Segments,
};
use std::{cell::RefCell, rc::Rc};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn cpu(code: &[u8], flags: u32, irq: &Rc<RefCell<Irq>>) -> Cpu {
    let mut state = CpuState {
        eip: 0x100,
        segments: Segments::real_mode(),
        ..Default::default()
    };
    state.registers.esp = 0x8000;
    state.flags.set_eflags(flags);
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        state,
        0x20000,
        PhysicalMemoryMap::new([(0..=0x1ffff, PhysicalMapping::Ram { backing_offset: 0 })])
            .unwrap(),
    )
    .unwrap();
    let memory = Memory::new(config.machine).unwrap();
    memory.view().write(0x100, code).unwrap();
    memory.view().write(0x180, &[0, 3, 0, 0]).unwrap();
    memory
        .view()
        .write(0x300, &[0xff, 0x06, 0, 5, 0xcf])
        .unwrap(); // INC word [0500]; IRET.
    Cpu::new(
        memory,
        config.cpu,
        IoBus::new([DeviceConnection {
            device: irq.clone(),
            mmio: vec![],
            ports: std::iter::once(0x80..=0x80).collect(),
        }]),
    )
    .await
    .unwrap()
}

async fn check_frames() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut cpu = cpu(&[0xeb, 0xfe], 0x40643, &irq).await;
    let state = cpu.cpu_state();
    state.set_segment(Segment::Cs, SegmentRegister::real_mode(Segment::Cs, 0x10));
    state.set_eip(0);
    state.set_segment(Segment::Ss, SegmentRegister::real_mode(Segment::Ss, 0x20));
    cpu.install_hooks([Hook::new(0x100, async || {
        HookResult::Exit(ExecutionExit::Host(HostExit::Return))
    })])
    .await
    .unwrap();
    irq.borrow_mut().raise();
    assert_eq!(cpu.run(10).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(irq.borrow().acknowledged, 1);
    assert_eq!(
        state.instruction_count(),
        2,
        "hardware entry retires no instruction"
    );
    assert_eq!(state.eip(), 0);
    assert_eq!(state.segment(Segment::Cs).selector, 0x10);
    assert_eq!(state.sp(), 0x8000);
    assert_eq!(
        state.eflags(),
        0x643,
        "IRET restores word flags; entry cleared AC"
    );
    let memory = cpu.memory();
    let mut frame = [0; 6];
    memory.view().read(0x81fa, &mut frame).unwrap();
    assert_eq!(frame, [0, 0, 0x10, 0, 0x43, 6]);
    assert_eq!(memory.view().read_u16(0x500).unwrap(), 1);
}

async fn check_entry_fault() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut cpu = cpu(&[0xeb, 0xfe], 0x202, &irq).await;
    let state = cpu.cpu_state();
    state.set_sp(4);
    let before = state.snapshot();
    irq.borrow_mut().raise();
    assert_eq!(
        cpu.run(1).await,
        ExecutionExit::Exception(Exception::StackFault { error_code: 0 })
    );
    assert_eq!(irq.borrow().acknowledged, 1);
    assert_eq!(state.snapshot(), before);
    state.set_sp(0x8000);
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(
        irq.borrow().acknowledged,
        1,
        "the fault does not acknowledge the vector twice"
    );
}

async fn check_live_if() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut cpu = cpu(&[0xeb, 0xfe], 2, &irq).await;
    irq.borrow_mut().raise();
    assert_eq!(cpu.run(0).await, ExecutionExit::BudgetExhausted);
    assert_eq!(irq.borrow().acknowledged, 0);
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(
        irq.borrow().acknowledged,
        0,
        "IF=0 leaves the request with its device"
    );
    cpu.cpu_state().set_interrupts_enabled(true);
    assert_eq!(cpu.run(0).await, ExecutionExit::BudgetExhausted);
    assert_eq!(
        irq.borrow().acknowledged,
        0,
        "zero budget never acknowledges a request"
    );
    assert_eq!(cpu.run(2).await, ExecutionExit::BudgetExhausted);
    assert_eq!(irq.borrow().acknowledged, 1);
}

macro_rules! test_backends {
    ($name:ident, $check:ident) => {
        #[cfg(not(target_arch = "wasm32"))]
        #[test]
        fn $name() {
            pollster::block_on($check());
        }
        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen_test::wasm_bindgen_test]
        async fn $name() {
            $check().await;
        }
    };
}

test_backends!(
    irq_entry_saves_current_position_and_returns_through_guest_iret,
    check_frames
);
test_backends!(
    irq_entry_fault_preserves_the_restart_boundary,
    check_entry_fault
);
test_backends!(
    queued_irqs_follow_live_if_and_zero_budget_does_no_work,
    check_live_if
);

#[path = "runtime_interrupts/protected_steps.rs"]
mod protected_steps;
