//! The resources borrowed by guest execution and its host services.

use super::{hooks::Handler, Backend, CpuContext, Dispatch, HookResult, RuntimeMemory};
use crate::IoBus;
use relocade_x86::ExecutionExit;
use std::{collections::HashMap, rc::Rc};

pub(super) struct Machine {
    pub backend: Backend,
    pub bindings: RuntimeMemory,
    pub io: IoBus,
    pub dispatch: Dispatch,
    pub hooks: HashMap<u32, Rc<dyn Handler>>,
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

    pub async fn call_host(&mut self, address: u32) -> HookResult {
        // The registration stays alive while its future borrows the machine.
        let handler = Rc::clone(self.hooks.get(&address).expect("registered host entry"));
        handler.call(&mut self.context()).await
    }
}
