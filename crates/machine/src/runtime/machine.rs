//! The resources borrowed by guest execution and its host services.

use super::{Backend, CpuContext, Dispatch, RuntimeMemory};
use crate::IoBus;
use relocade_x86::ExecutionExit;

pub(super) struct Machine {
    pub backend: Backend,
    pub bindings: RuntimeMemory,
    pub io: IoBus,
    pub dispatch: Dispatch,
}

impl Machine {
    pub fn context(&mut self) -> CpuContext<'_> {
        CpuContext {
            cpu: &self.bindings.cpu,
            memory: self.bindings.guest.view().with_io(&self.io),
            io: &self.io,
            interrupt_calls: None,
        }
    }

    pub fn dispatch(&mut self) -> ExecutionExit {
        self.dispatch.run(&mut self.backend)
    }
}
