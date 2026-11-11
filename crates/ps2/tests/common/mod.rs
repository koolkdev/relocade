use relocade_machine::{
    worker::{wake_connection, WakeHost},
    Clock, CpuConfig, ExecutionInterrupt, Machine, MachineConfig, Runner,
};
use relocade_x86::{Compiler, CpuState, ExecutionProfile, PhysicalMemoryMap, Segments};

// Device activities run locally with a manual clock; these fixtures never enter a CPU.
pub async fn runner(
    clock: Clock,
    compiler: &relocade_machine::CompilerService,
) -> (Runner, WakeHost<ExecutionInterrupt>, Machine) {
    let mut machine = Machine::new(
        MachineConfig::new(0, PhysicalMemoryMap::new([]).unwrap()).unwrap(),
        compiler,
    )
    .unwrap();
    let cpu = CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            segments: Segments::real_mode(),
            ..Default::default()
        },
    )
    .unwrap();
    let prepared = machine.create_cpu(cpu).unwrap();
    let (host, endpoint) = wake_connection(prepared.interrupt_handle());
    (
        Runner::new(clock, prepared, endpoint).await.unwrap(),
        host,
        machine,
    )
}
