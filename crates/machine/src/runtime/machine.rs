//! The resources borrowed by guest execution and its host services.

use super::{hooks::Handler, Backend, CpuContext, Dispatch, RuntimeMemory};
use crate::{CpuView, IoBus, MemoryBus};
use relocade_wasm_runtime::WasmMemoryAccess;
use relocade_x86::ExecutionExit;
use std::{collections::HashMap, rc::Rc};

pub(super) struct Machine<T> {
    pub backend: Backend,
    pub bindings: RuntimeMemory,
    pub io: IoBus,
    pub memory: MemoryBus,
    pub state: T,
    pub dispatch: Dispatch,
    pub hooks: HashMap<u32, Rc<dyn Handler<T>>>,
}

impl<T> Machine<T> {
    pub fn context(&mut self) -> CpuContext<'_, T> {
        CpuContext {
            cpu: CpuView::new(
                self.bindings.config.execution_target(),
                self.backend.memory(&self.bindings.context),
            ),
            memory: self.memory.view(),
            io: &self.io,
            state: &mut self.state,
        }
    }

    pub fn dispatch(&mut self, budget: Option<u64>) -> ExecutionExit {
        if let Some(budget) = budget {
            let target = self
                .context()
                .cpu
                .instruction_count()
                .saturating_add(budget);
            self.bindings
                .set_instruction_target(&mut self.backend, target);
        }
        self.dispatch.run(&mut self.backend)
    }
}
