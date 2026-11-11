//! Retained Wasm runtime access shared by the CPU and synchronous host I/O.

use super::{Result, WasmRuntime};
use std::{
    cell::{OnceCell, RefCell},
    rc::Rc,
};

/// Clones keep one runtime alive on its execution thread. Create it lazily so
/// constructing an I/O bus does not require an engine before its first use.
#[derive(Clone, Default)]
pub(crate) struct WasmRuntimeHandle {
    runtime: Rc<OnceCell<RefCell<WasmRuntime>>>,
}

impl WasmRuntimeHandle {
    /// Creates the runtime if needed, reporting engine initialization failures.
    pub(crate) fn initialize(&self) -> Result<()> {
        self.get_or_create().map(|_| ())
    }

    /// Borrows the runtime exclusively for one synchronous operation. Release
    /// the borrow before polling asynchronous services or device activities.
    /// Engine callbacks use their supplied context within this same borrow.
    pub(crate) fn with_runtime<R>(&self, access: impl FnOnce(&mut WasmRuntime) -> R) -> R {
        let runtime = self.get_or_create().expect("create Wasm runtime");
        access(&mut runtime.borrow_mut())
    }

    fn get_or_create(&self) -> Result<&RefCell<WasmRuntime>> {
        if self.runtime.get().is_none() {
            let runtime = RefCell::new(WasmRuntime::new()?);
            assert!(self.runtime.set(runtime).is_ok());
        }
        Ok(self.runtime.get().unwrap())
    }
}
