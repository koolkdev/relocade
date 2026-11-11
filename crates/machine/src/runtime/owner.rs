//! Retains engine resources independently of the CPU's execution lifetime.

use super::{Backend, Result};
use std::{
    cell::{Cell, OnceCell, RefCell},
    rc::Rc,
};

/// Shared by the execution controller and retained synchronous host access.
/// Engine callbacks run within the current engine access scope.
#[derive(Clone, Default)]
pub(crate) struct Runtime(Rc<State>);

#[derive(Default)]
struct State {
    backend: OnceCell<RefCell<Backend>>,
    attached: Cell<bool>,
}

impl Runtime {
    /// Claims the only execution controller, even if setup subsequently fails.
    pub(crate) fn attach_cpu(&self) -> Result<()> {
        assert!(
            !self.0.attached.replace(true),
            "an I/O bus supports exactly one CPU"
        );
        self.backend()?;
        Ok(())
    }

    /// Acquires the engine for one synchronous operation. The borrow cannot
    /// escape into a future; release it before polling asynchronous services or
    /// device activities.
    pub(crate) fn with_backend<R>(&self, access: impl FnOnce(&mut Backend) -> R) -> R {
        let backend = self.backend().expect("create machine runtime");
        access(&mut backend.borrow_mut())
    }

    fn backend(&self) -> Result<&RefCell<Backend>> {
        if self.0.backend.get().is_none() {
            let backend = RefCell::new(Backend::new()?);
            assert!(self.0.backend.set(backend).is_ok());
        }
        Ok(self.0.backend.get().unwrap())
    }
}
