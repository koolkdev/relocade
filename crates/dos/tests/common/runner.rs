#[path = "../../../machine/tests/support/compilation.rs"]
pub mod compilation;

use relocade_machine::{
    worker::{wake_connection, WakeHost},
    Clock, ExecutionInterrupt, Machine, Runner,
};

// Native service fixtures use the same initialized session on their test thread.
// Retain the machine and host while driving the runner.
pub async fn runner(
    clock: Clock,
    compiler: &relocade_machine::CompilerService,
) -> (Runner, WakeHost<ExecutionInterrupt>, Machine) {
    let mut machine = relocade_machine::Machine::new(relocade_pc::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_pc::configuration::cpu(), compiler)
        .unwrap();
    let (host, endpoint) = wake_connection(prepared.interrupt_handle());
    (
        Runner::new(clock, prepared, endpoint).await.unwrap(),
        host,
        machine,
    )
}
