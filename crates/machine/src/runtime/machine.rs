//! The resources borrowed by guest execution and its host services.

use super::{hooks::Handler, Backend, CpuContext, Dispatch, HookResult, RuntimeMemory};
use crate::CpuView;
use relocade_wasm_runtime::WasmMemoryAccess;
use relocade_x86::ExecutionExit;
use std::{collections::HashMap, rc::Rc};

pub(super) struct Machine<T> {
    pub backend: Backend,
    pub bindings: RuntimeMemory,
    pub state: T,
    pub dispatch: Dispatch,
    pub hooks: HashMap<u32, Rc<dyn Handler<T>>>,
}

impl<T> Machine<T> {
    pub fn context(&mut self) -> CpuContext<'_, T> {
        CpuContext {
            cpu: CpuView::new(
                &self.bindings.config,
                self.backend.memory(&self.bindings.context),
            ),
            memory: self.bindings.guest.view(),
            io: self.bindings.guest.io(),
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

    pub async fn call_host(&mut self, address: u32) -> HookResult {
        // The registration stays alive while its future borrows the machine.
        let handler = Rc::clone(self.hooks.get(&address).expect("registered host entry"));
        handler.call(&mut self.context()).await
    }
}
