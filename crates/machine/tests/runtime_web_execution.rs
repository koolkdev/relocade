#![cfg(target_arch = "wasm32")]
mod support;

use relocade_x86::{
    Compiler, CpuFeature, CpuModel, CpuState, Exception, ExecutionExit, ExecutionProfile,
    PhysicalMapping, PhysicalMemoryMap,
};

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test::wasm_bindgen_test]
async fn runtime_executes_with_live_cpu_and_memory() {
    let target = Compiler::new(ExecutionProfile::Real16)
        .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Tsc, false))
        .target();
    let cpu = CpuState {
        eip: 0x100,
        ..CpuState::real_mode()
    };
    let map =
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap();
    let config = support::configuration(target, cpu, 0x10000, map).unwrap();
    let mut runtime = support::create_cpu(config).await.unwrap();
    runtime
        .context()
        .memory
        .write(0x100, &[0x40, 0xeb, 0xfd]) // INC AX; JMP 0100
        .unwrap();
    runtime.context().cpu.set_eax(41);
    support::steps(&mut runtime, 2);
    assert_eq!(runtime.context().cpu.eax(), 42);
    assert_eq!(runtime.context().cpu.eip(), 0x100);
    runtime.context().cpu.set_eax(100);
    support::steps(&mut runtime, 2);
    assert_eq!(runtime.context().cpu.eax(), 101);
    assert_eq!(runtime.context().cpu.instruction_count(), 4);
    // The same backing model controls CPUID and instruction admission.
    runtime
        .context()
        .memory
        .write(0x100, &[0x0f, 0xa2, 0x0f, 0x31])
        .unwrap();
    runtime.context().cpu.set_eip(0x100);
    runtime.context().cpu.set_eax(1);
    support::steps(&mut runtime, 1);
    assert_eq!(runtime.context().cpu.edx(), 0x0788_8101);
    runtime.cpu_state().set_sp(0x8000);
    relocade_machine::real_mode_interrupt::write_vector(
        &mut runtime.memory().view(),
        Exception::InvalidOpcode.vector() as u8,
        relocade_machine::FarPointer {
            segment: 0,
            offset: 0x600,
        },
    );
    let before = runtime.cpu_state().snapshot();
    assert_eq!(runtime.step(), ExecutionExit::InterruptEntered);
    assert_eq!(runtime.cpu_state().eip(), 0x600);
    assert_eq!(
        runtime.cpu_state().instruction_count(),
        before.instruction_count
    );
    let frame = relocade_machine::real_mode_interrupt::read_frame(
        &runtime.cpu_state(),
        &runtime.memory().view(),
    );
    assert_eq!(u32::from(frame.return_address.offset), before.eip);
}
