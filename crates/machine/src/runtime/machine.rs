//! The resources borrowed by guest execution and its host services.

use super::{hooks::Service, Backend, CpuContext, Dispatch, RuntimeMemory};
use crate::{CpuView, IoBus};
use relocade_wasm_runtime::WasmMemoryAccess;
use relocade_x86::ExecutionExit;
use std::collections::HashMap;

pub(super) struct Machine<T> {
    pub backend: Backend,
    pub bindings: RuntimeMemory,
    pub io: IoBus,
    pub state: T,
    pub dispatch: Dispatch,
    pub hooks: HashMap<u32, Service<T>>,
}

impl<T> Machine<T> {
    pub fn context(&mut self) -> CpuContext<'_, T> {
        CpuContext {
            cpu: CpuView::new(
                &self.bindings.config,
                self.backend.memory(&self.bindings.context),
            ),
            memory: self.bindings.guest.view().with_io(&self.io),
            io: &self.io,
            state: &mut self.state,
        }
    }

    fn set_budget(&mut self, budget: Option<u64>) {
        if let Some(budget) = budget {
            let target = self
                .context()
                .cpu
                .instruction_count()
                .saturating_add(budget);
            self.bindings
                .set_instruction_target(&mut self.backend, target);
        }
    }

    pub fn dispatch(&mut self, budget: Option<u64>) -> ExecutionExit {
        self.set_budget(budget);
        let exit = self.dispatch.run(&mut self.backend);
        self.step_protected(exit)
    }

    fn step_protected(&mut self, mut exit: ExecutionExit) -> ExecutionExit {
        while exit == ExecutionExit::InterruptInhibited {
            exit = self.dispatch.step(&mut self.backend);
        }
        exit
    }

    pub fn interrupt(&mut self, vector: u8, budget: Option<u64>) -> ExecutionExit {
        self.set_budget(budget);
        let exit = self.dispatch.interrupt(&mut self.backend, vector);
        self.step_protected(exit)
    }
}
