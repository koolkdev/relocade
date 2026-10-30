//! The resources borrowed by guest execution and its host services.

use super::{hooks::Handler, Backend, CpuContext, Dispatch, RuntimeMemory};
use crate::{CpuView, IoBus};
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
        let mut exit = match self.poll_interrupt(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(vector) => self.dispatch.interrupt(&mut self.backend, vector),
            Poll::Pending => self.dispatch.run(&mut self.backend),
        };
        while exit == ExecutionExit::InterruptInhibited {
            exit = self.dispatch.step(&mut self.backend);
        }
        exit
    }

    pub fn poll_interrupt(&mut self, cx: &mut Context<'_>) -> Poll<u8> {
        if !self.context().cpu.interrupts_enabled() {
            return Poll::Pending;
        }
        self.io.poll_interrupt(cx)
    }
}
