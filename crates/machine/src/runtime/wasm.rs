//! Worker-local Wasm runtime retained by the CPU and its attached devices.

use super::{Result, WasmRuntime};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
pub(crate) struct WasmRuntimeHandle(Rc<RefCell<WasmRuntime>>);

impl WasmRuntimeHandle {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self(Rc::new(RefCell::new(WasmRuntime::new()?))))
    }

    /// Borrows the runtime exclusively for one synchronous operation. Release
    /// the borrow before polling asynchronous services or device activities.
    /// Engine callbacks use their supplied context within this same borrow.
    pub(crate) fn with_runtime<R>(&self, access: impl FnOnce(&mut WasmRuntime) -> R) -> R {
        access(&mut self.0.borrow_mut())
    }
}
