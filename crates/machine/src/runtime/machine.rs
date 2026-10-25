//! The resources borrowed by guest execution and its host services.

use super::{hooks::Handler, Backend, CpuContext, Dispatch, HookResult, RuntimeMemory};
use relocade_x86::ExecutionExit;
use std::{cell::Cell, collections::HashMap, rc::Rc};

pub(super) struct Machine<T> {
    pub backend: Backend,
    pub bindings: RuntimeMemory,
    pub state: T,
    pub dispatch: Dispatch,
    pub hooks: HashMap<u32, Rc<dyn Handler<T>>>,
}

impl<T> Machine<T> {
    pub fn context(&mut self) -> CpuContext<'_, T> {
        CpuContext { machine: self }
    }

    pub fn dispatch(&mut self, budget: Option<u64>) -> ExecutionExit {
        if let Some(budget) = budget {
            let target = self
                .context()
                .cpu()
                .instruction_count()
                .saturating_add(budget);
            self.bindings
                .set_instruction_target(&mut self.backend, target);
        }
        self.dispatch.run(&mut self.backend)
    }

    pub async fn resume_host_call(
        &mut self,
        mut address: u32,
        budget: &Cell<Option<u64>>,
    ) -> ExecutionExit {
        loop {
            // The registration stays alive while its future borrows the machine.
            let handler = Rc::clone(self.hooks.get(&address).expect("registered host entry"));
            match handler.call(&mut self.context()).await {
                HookResult::Continue => {}
                HookResult::Exit(exit) => return exit,
            }
            // A new waiter supplies one budget. Suspension does not replenish it.
            // A replacement budget takes effect when the pending service returns.
            match self.dispatch(budget.take()) {
                ExecutionExit::HostCall { address: next } => address = next,
                exit => return exit,
            }
        }
    }
}
