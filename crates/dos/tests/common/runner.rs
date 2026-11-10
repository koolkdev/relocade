use relocade_machine::{
    worker::{wake_connection, WakeHost},
    Clock, ExecutionInterrupt, Runner,
};

// Native service fixtures use the same initialized session on their test thread.
// Retain the host while driving the runner; CPU-only fixtures may drop both after setup.
pub async fn runner(clock: Clock) -> (Runner, WakeHost<ExecutionInterrupt>) {
    let mut machine = relocade_machine::Machine::new(relocade_pc::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_pc::configuration::cpu())
        .unwrap();
    let (host, endpoint) = wake_connection(prepared.interrupt_handle());
    (Runner::new(clock, prepared, endpoint).await.unwrap(), host)
}
