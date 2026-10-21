#![cfg(target_arch = "wasm32")]

use relocade_machine::MachineConfig;
use relocade_machine::{Cpu, RuntimeState};
use relocade_wasm_runtime::Backend;
use relocade_x86::{
    Compiler, CpuFeature, CpuModel, CpuState, Exception, ExecutionExit, ExecutionProfile,
    PhysicalMapping, PhysicalMemoryMap, Segments,
};

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test::wasm_bindgen_test]
async fn runtime_executes_with_live_cpu_and_memory() {
    let target = Compiler::new(ExecutionProfile::Real16)
        .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Tsc, false))
        .target();
    let cpu = CpuState {
        eip: 0x100,
        segments: Segments::real_mode(),
        ..CpuState::default()
    };
    let map =
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap();
    let config = MachineConfig::new(target, cpu, 0x10000, map).unwrap();
    let mut runtime = Cpu::new(Backend::with_state(RuntimeState::new(())).unwrap(), config)
        .await
        .unwrap();
    runtime
        .context()
        .memory()
        .write(0x100, &[0x40, 0xeb, 0xfd]) // INC AX; JMP 0100
        .unwrap();
    runtime.context().cpu().set_eax(41);
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu().eax(), 42);
    assert_eq!(runtime.context().cpu().eip(), 0x100);
    runtime.context().cpu().set_eax(100);
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu().eax(), 101);
    assert_eq!(runtime.context().cpu().instruction_count(), 4);
    // The same backing model controls CPUID and instruction admission.
    runtime
        .context()
        .memory()
        .write(0x100, &[0x0f, 0xa2, 0x0f, 0x31])
        .unwrap();
    runtime.context().cpu().set_eip(0x100);
    runtime.context().cpu().set_eax(1);
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu().edx(), 0x0788_8101);
    let before = runtime.context().cpu().snapshot();
    assert_eq!(
        runtime.run(1).unwrap(),
        ExecutionExit::Exception(Exception::InvalidOpcode)
    );
    assert_eq!(runtime.context().cpu().snapshot(), before);
}
