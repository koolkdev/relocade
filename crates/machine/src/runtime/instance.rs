//! The resources borrowed by guest execution and its host services.

use super::{CpuContext, Dispatch, Runtime, RuntimeMemory};
use crate::IoBus;
use relocade_x86::ExecutionExit;

pub(super) struct CpuInstance {
    pub runtime: Runtime,
    pub bindings: RuntimeMemory,
    pub io: IoBus,
    pub dispatch: Dispatch,
}

impl CpuInstance {
    pub fn context(&mut self) -> CpuContext<'_> {
        CpuContext {
            cpu: &self.bindings.cpu,
            memory: self.bindings.guest.view().with_io(&self.io),
            io: &self.io,
            guest_calls: None,
        }
    }

    pub fn dispatch(&mut self) -> ExecutionExit {
        self.runtime
            .with_backend(|backend| self.dispatch.run(backend))
    }
}
