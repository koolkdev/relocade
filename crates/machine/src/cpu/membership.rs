use crate::machine::Shared;
use std::sync::{atomic::Ordering, Arc};

/// Identifies a CPU within its machine. IDs are not reused after a CPU is dropped.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CpuId(usize);

pub(crate) struct Membership {
    pub id: CpuId,
    machine: Arc<Shared>,
}

impl Membership {
    pub fn new(machine: &Arc<Shared>) -> Arc<Self> {
        let id = machine
            .next_cpu
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("CPU ID capacity");
        machine.cpus.fetch_add(1, Ordering::Relaxed);
        Arc::new(Self {
            id: CpuId(id),
            machine: Arc::clone(machine),
        })
    }
}

impl Drop for Membership {
    fn drop(&mut self) {
        self.machine.cpus.fetch_sub(1, Ordering::Relaxed);
    }
}
