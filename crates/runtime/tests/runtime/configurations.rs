use super::*;
use wasm86_x86::{
    Compiler, CpuFeature, CpuModel, CpuState, Exception, ExecutionExit, ExecutionTarget,
    PagedMemoryMap, SegmentProfile,
};

fn flat_machine<E: Environment>(environment: E) -> Machine<E> {
    flat_machine_for(environment, Compiler::new(SegmentProfile::Flat32).target())
}
fn flat_machine_for<E: Environment>(environment: E, target: ExecutionTarget) -> Machine<E> {
    let mut map = PagedMemoryMap::default();
    map.map(0x1000..=0x1fff, 0, true).unwrap();
    map.map(0x80001000..=0x80001fff, 0x2000, true).unwrap();
    let cpu = CpuState {
        eip: 0x1000,
        ..CpuState::default()
    };
    Machine::new(target, cpu, map, environment).unwrap()
}

#[pollster::test]
async fn one_runtime_executes_real_and_flat_machines_with_private_backing() {
    let runtime = runtime().await;
    let mut real = runtime.attach(real_machine(ram(), ())).await.unwrap();
    real.write_memory(0, &[0xb8, 7, 0, 0xeb, 0]).unwrap();
    // MOV EAX, [80001000h] accesses high linear addresses through remapped backing.
    let mut flat = runtime.attach(flat_machine(())).await.unwrap();
    flat.write_memory(0x1000, &[0xa1, 0, 0x10, 0, 0x80, 0xeb, 0])
        .unwrap();
    flat.write_memory(0x80001000, &42u32.to_le_bytes()).unwrap();
    assert_eq!(flat.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(flat.cpu().registers.eax, 42);
    assert_eq!(real.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(real.cpu().registers.eax, 7);
    assert_eq!(flat.cpu().eip, 0x1007);
    assert_eq!(real.cpu().eip, 5);
}

#[pollster::test]
async fn prepared_code_preserves_cpu_capabilities() {
    let runtime = runtime().await;
    let code = [0x0f, 0x44, 0xc0, 0xeb, 0]; // CMOVE EAX,EAX; JMP to the next block
    let mut enabled = runtime.attach(flat_machine(())).await.unwrap();
    enabled.write_memory(0x1000, &code).unwrap();
    let compiler = Compiler::new(SegmentProfile::Flat32)
        .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Cmov, false));
    let mut disabled = runtime
        .attach(flat_machine_for((), compiler.target()))
        .await
        .unwrap();
    disabled.write_memory(0x1000, &code).unwrap();
    assert_eq!(enabled.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(
        disabled.run(1).unwrap(),
        ExecutionExit::Exception(Exception::InvalidOpcode)
    );
    assert_eq!(disabled.cpu().eip, 0x1000);
}
