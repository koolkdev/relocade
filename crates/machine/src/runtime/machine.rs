//! The resources borrowed by guest execution and its host services.

use super::{services::Services, Backend, CpuContext, Dispatch, RuntimeMemory};
use crate::IoBus;
use relocade_x86::ExecutionExit;

pub(super) struct Machine {
    pub backend: Backend,
    pub bindings: RuntimeMemory,
    pub io: IoBus,
    pub dispatch: Dispatch,
    pub services: Services,
}

impl Machine {
    pub fn context(&mut self) -> CpuContext<'_> {
        CpuContext {
            cpu: &self.bindings.cpu,
            memory: self.bindings.guest.view().with_io(&self.io),
            io: &self.io,
        }
    }

    pub fn dispatch(&mut self, budget: Option<u64>) -> ExecutionExit {
        if let Some(budget) = budget {
            let target = self.bindings.cpu.instruction_count().saturating_add(budget);
            self.bindings.cpu.set_instruction_target(target);
        }
        self.dispatch.run(&mut self.backend)
    }
}
