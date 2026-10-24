//! Live machine resources, independent of the futures executing its services.

use super::{Backend, RuntimeMemory};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub(super) struct MachineCore<T> {
    pub backend: RefCell<Backend>,
    pub bindings: Rc<RuntimeMemory>,
    pub state: RefCell<T>,
    // The runner controls this access gate. A service may only touch its own
    // active guest context; stopped embedding handles have no call identity.
    pub active_call: Cell<Option<u64>>,
    pub running: Cell<bool>,
    pub closed: Cell<bool>,
}

impl<T> MachineCore<T> {
    pub fn validate(&self, invocation: Option<u64>) {
        assert!(!self.closed.get(), "CPU has been dropped");
        assert!(!self.running.get(), "guest execution is active");
        if let Some(invocation) = invocation {
            assert_eq!(
                self.active_call.get(),
                Some(invocation),
                "host call is no longer active"
            );
        }
    }
}
