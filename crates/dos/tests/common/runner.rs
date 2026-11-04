use relocade_machine::{
    worker::{wake_connection, WakeHost},
    Clock, CpuStorage, ExecutionInterrupt, Runner,
};

// Native service fixtures use the same initialized session on their test thread.
// Retain the host while driving the runner; CPU-only fixtures may drop both after setup.
pub fn runner<C: Clock>(clock: C) -> (Runner<C>, WakeHost<ExecutionInterrupt>) {
    let storage = CpuStorage::new().unwrap();
    let (host, endpoint) = wake_connection(storage.interrupt_handle());
    (Runner::new(clock, storage, endpoint), host)
}
