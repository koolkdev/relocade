use relocade_machine::{
    worker::{wake_connection, WakeHost},
    Clock, CpuStorage, ExecutionInterrupt, Runner,
};

// Native service fixtures use the same initialized session on their test thread.
// Retain the host while driving the runner; CPU-only fixtures may drop both after setup.
pub fn runner(clock: Clock) -> (Runner, WakeHost<ExecutionInterrupt>) {
    let storage = CpuStorage::new().unwrap();
    let (host, endpoint) = wake_connection(storage.interrupt_handle());
    (Runner::new(clock, storage, endpoint), host)
}
