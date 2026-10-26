//! The resources borrowed by guest execution and its host services.

use super::{callback::DeferredCall, hooks::Handler, Backend, CpuContext, Dispatch, RuntimeMemory};
use crate::CpuView;
use relocade_wasm_runtime::WasmMemoryAccess;
use std::{collections::HashMap, rc::Rc};

pub(super) struct Machine<T> {
    pub backend: Backend,
    pub bindings: RuntimeMemory,
    pub state: T,
    pub dispatch: Dispatch,
    pub hooks: HashMap<u32, Rc<Handler<T>>>,
    pub pending: Option<Box<dyn DeferredCall<T>>>,
}

impl<T> Machine<T> {
    pub fn context(&mut self) -> CpuContext<'_, T> {
        CpuContext {
            cpu: CpuView::new(
                &self.bindings.config,
                self.backend.memory(&self.bindings.context),
            ),
            memory: self.bindings.guest.view(),
            state: &mut self.state,
            pending: &mut self.pending,
        }
    }
}
