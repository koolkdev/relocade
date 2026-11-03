use super::{support, Adjustment};
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_handler, Interrupts},
    FarPointer,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};
use std::{cell::Cell, rc::Rc};

#[interrupt(args(value = ax), returns { value => { ax = value, cf = false } })]
async fn advance(value: u16, increment: &Cell<u16>) -> u16 {
    increment.set(increment.get() + 1);
    value + increment.get()
}

async fn check_handlers() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let mut runtime = support::create_cpu(config).await.unwrap();
    runtime.context().cpu.set_sp(0xff00);
    runtime.context().cpu.set_ax(5);
    runtime
        .context()
        .memory
        .write(
            0x100,
            &[
                0xcd, 0x60, // INT 60h: store BX
                0xcd, 0x61, // INT 61h: advance AX
                0xcd, 0x62, // INT 62h: adjust AX
                0xeb, 0xf8, // JMP 0100
            ],
        )
        .unwrap();

    // Captures may be non-Send and share a name with the generated context.
    let call = Rc::new(Cell::new(0));
    let calls = call.clone();
    let adjustment = Adjustment(0x10);
    let mut interrupts = Interrupts::new(&mut runtime);
    interrupts.register_interrupt(
        0x60,
        FarPointer {
            segment: 0,
            offset: 0x400,
        },
        interrupt_handler!(super::store_value),
    );
    interrupts.register_interrupt(
        0x61,
        FarPointer {
            segment: 0,
            offset: 0x410,
        },
        interrupt_handler!(advance(&call)),
    );
    interrupts.register_interrupt(
        0x62,
        FarPointer {
            segment: 0,
            offset: 0x420,
        },
        interrupt_handler!(adjustment.add(3)),
    );
    interrupts.install().await.unwrap();

    // Each registration is reusable and reads the registers of the new call.
    for (invocations, value) in [(1, 0x19), (2, 0x2e)] {
        runtime.context().cpu.set_bx(invocations);
        runtime.context().cpu.set_carry(true);
        assert_eq!(runtime.run(7).await, ExecutionExit::BudgetExhausted);
        assert_eq!(calls.get(), invocations);
        assert_eq!(runtime.context().cpu.ax(), value);
        assert!(!runtime.context().cpu.carry());
        assert_eq!(runtime.context().cpu.eip(), 0x100);
        assert_eq!(runtime.context().cpu.sp(), 0xff00);
        assert_eq!(
            runtime.context().memory.read_u16(0x302).unwrap(),
            invocations
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn registered_bindings_reuse_captures_and_apply_service_abis() {
    pollster::block_on(check_handlers());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn registered_bindings_reuse_captures_and_apply_service_abis() {
    check_handlers().await;
}
