use relocade_machine::{
    worker::{wake_connection, WakeHost},
    Clock, CpuConfig, ExecutionInterrupt, Machine, MachineConfig, Runner,
};
use relocade_x86::{CpuState, PhysicalMapping, PhysicalMemoryMap};

// One execution owner with a manual clock and a page for guest I/O programs.
pub async fn runner(clock: Clock) -> (Runner, WakeHost<ExecutionInterrupt>, Machine) {
    let mut machine = Machine::new(
        MachineConfig::new(
            4096,
            PhysicalMemoryMap::new([(0..=4095, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let cpu = CpuConfig::pc(CpuState {
        eip: 0x100,
        ..CpuState::real_mode()
    })
    .unwrap();
    let prepared = machine.create_cpu(cpu, None).unwrap();
    let (host, endpoint) = wake_connection(prepared.interrupt_handle());
    (
        Runner::new(clock, prepared, endpoint).await.unwrap(),
        host,
        machine,
    )
}
