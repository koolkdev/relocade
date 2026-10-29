//! The resources borrowed by guest execution and its host services.

use super::{hooks::Handler, Backend, CpuContext, Dispatch, HookResult, RuntimeMemory};
use crate::{CpuView, InterruptSource, IoBus};
use relocade_wasm_runtime::WasmMemoryAccess;
use relocade_x86::ExecutionExit;
use std::{
    collections::HashMap,
    rc::Rc,
    task::{Context, Poll, Waker},
};

pub(super) struct Machine<T> {
    pub backend: Backend,
    pub bindings: RuntimeMemory,
    pub io: IoBus,
    pub state: T,
    pub dispatch: Dispatch,
    pub hooks: HashMap<u32, Rc<dyn Handler<T>>>,
    pub interrupts: Option<Box<dyn InterruptSource>>,
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
        // This call does not suspend; each return to guest dispatch polls again.
        let vector = match self.poll_interrupt(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(vector) => Some(vector),
            Poll::Pending => None,
        };
        self.enter(vector)
    }

    fn enter(&mut self, vector: Option<u8>) -> ExecutionExit {
        let mut exit = match vector {
            Some(vector) => self.dispatch.interrupt(&mut self.backend, vector),
            None => self.dispatch.run(&mut self.backend),
        };
        while exit == ExecutionExit::InterruptInhibited {
            exit = self.dispatch.step(&mut self.backend);
        }
        exit
    }

    pub fn poll_interrupt(&mut self, cx: &mut Context<'_>) -> Poll<u8> {
        if self.interrupts.is_none() || !self.context().cpu.interrupts_enabled() {
            return Poll::Pending;
        }
        self.interrupts.as_mut().unwrap().poll_interrupt(cx)
    }

    pub async fn call_host(&mut self, address: u32) -> HookResult {
        // The registration stays alive while its future borrows the machine.
        let handler = Rc::clone(self.hooks.get(&address).expect("registered host entry"));
        handler.call(&mut self.context()).await
    }
}
