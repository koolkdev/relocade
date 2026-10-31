//! The resources borrowed by guest execution and its host services.

use super::{hooks::Handler, Backend, CpuContext, Dispatch, RuntimeMemory};
use crate::{CpuView, IoBus};
use relocade_wasm_runtime::WasmMemoryAccess;
use relocade_x86::ExecutionExit;
use std::{collections::HashMap, rc::Rc};

pub(super) struct Machine {
    pub backend: Backend,
    pub bindings: RuntimeMemory,
    pub io: IoBus,
    pub dispatch: Dispatch,
    pub hooks: Vec<Option<Rc<dyn Handler>>>,
    pub hook_ids: HashMap<u32, u32>,
}

impl Machine {
    pub fn context(&mut self) -> CpuContext<'_> {
        CpuContext {
            cpu: CpuView::new(
                self.bindings.config.execution_target(),
                self.backend.memory(&self.bindings.context).into_prefix(),
            ),
            memory: self.bindings.guest.view().with_io(&self.io),
            io: &self.io,
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
