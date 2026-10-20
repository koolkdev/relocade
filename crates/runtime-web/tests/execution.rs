#![cfg(target_arch = "wasm32")]

use relocade_machine::MachineConfig;
use relocade_runtime::{Runtime, RuntimeState};
use relocade_runtime_web::WebBackend;
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test::wasm_bindgen_test]
async fn runtime_executes_with_live_cpu_and_memory() {
    let target = Compiler::new(ExecutionProfile::Real16).target();
    let cpu = CpuState {
        eip: 0x100,
        segments: Segments::real_mode(),
        ..CpuState::default()
    };
    let map =
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap();
    let config = MachineConfig::new(target, cpu, 0x10000, map).unwrap();
    let mut runtime = Runtime::new(WebBackend::with_state(RuntimeState::new(())), config)
        .await
        .unwrap();
    runtime
        .machine()
        .memory()
        .write(0x100, &[0x40, 0xeb, 0xfd]) // INC AX; JMP 0100
        .unwrap();
    runtime.machine().cpu().set_eax(41);
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.machine().cpu().eax(), 42);
    assert_eq!(runtime.machine().cpu().eip(), 0x100);
    runtime.machine().cpu().set_eax(100);
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.machine().cpu().eax(), 101);
    assert_eq!(runtime.machine().cpu().instruction_count(), 4);
}
