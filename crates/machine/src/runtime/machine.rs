//! The resources borrowed by guest execution and its host services.

use super::{services::Services, Backend, CpuContext, Dispatch, RuntimeMemory};
use crate::IoBus;
use relocade_x86::{ExecutionExit, ExecutionProfile};
use std::task::{Context, Poll};

pub(super) struct Machine {
    pub backend: Backend,
    pub bindings: RuntimeMemory,
    pub io: IoBus,
    pub dispatch: Dispatch,
    pub services: Services,
}

impl Machine {
    /// Poll only at stopped execution boundaries after protected instructions,
    /// using live IF. Empty buses need no CPU-state reads.
    #[inline]
    pub fn poll_interrupt(&self, cx: &mut Context<'_>) -> Poll<u8> {
        let cpu = &self.bindings.cpu;
        if self.io.is_empty()
            || cpu.execution_target().profile != ExecutionProfile::Real16
            || !cpu.interrupts_enabled()
        {
            return Poll::Pending;
        }
        self.io.poll_interrupt(cx)
    }

    pub fn enter_interrupt(&mut self, vector: u8) -> Result<(), ExecutionExit> {
        match self.dispatch.enter_interrupt(&mut self.backend, vector) {
            ExecutionExit::InterruptEntered => Ok(()),
            exit => Err(exit),
        }
    }

    pub fn context(&mut self) -> CpuContext<'_> {
        CpuContext {
            cpu: &self.bindings.cpu,
            memory: self.bindings.guest.view().with_io(&self.io),
            io: &self.io,
        }
    }

    #[inline]
    pub fn dispatch(&mut self, budget: Option<u64>) -> ExecutionExit {
        if let Some(budget) = budget {
            let target = self.bindings.cpu.instruction_count().saturating_add(budget);
            self.bindings.cpu.set_instruction_target(target);
        }
        self.dispatch.run(&mut self.backend)
    }
}
