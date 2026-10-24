//! Owned access handles whose operations borrow live machine storage briefly.

mod storage;
use super::{core::MachineCore, RuntimeMemory};
use crate::{CpuView, MemoryView};
use relocade_wasm_runtime::MemoryAccess;
use relocade_x86::ExecutionTarget;
use std::rc::Rc;
use storage::{ContextMemory, Region};

/// Owned access to CPU state, guest memory and environment data. CPU and memory
/// views retain this handle, not a backend borrow, and may remain across awaits.
/// A service handle is valid only while its invocation is active. Completing or
/// cancelling that call, or dropping the CPU, revokes all copies of its handle.
pub struct CpuContext<T = ()> {
    bindings: Rc<RuntimeMemory>,
    cpu: ContextMemory<T>,
    guest: ContextMemory<T>,
}

impl<T> Clone for CpuContext<T> {
    fn clone(&self) -> Self {
        Self {
            bindings: Rc::clone(&self.bindings),
            cpu: self.cpu.clone(),
            guest: self.guest.clone(),
        }
    }
}

impl<T> CpuContext<T> {
    pub(super) fn new(core: &Rc<MachineCore<T>>, invocation: Option<u64>) -> Self {
        Self {
            bindings: Rc::clone(&core.bindings),
            cpu: ContextMemory::new(core, invocation, Region::Cpu),
            guest: ContextMemory::new(core, invocation, Region::Guest),
        }
    }

    pub fn execution_target(&self) -> ExecutionTarget {
        self.bindings.config.execution_target()
    }

    #[inline]
    pub fn cpu(&self) -> CpuView<'_> {
        CpuView::new(
            &self.bindings.config,
            MemoryAccess::from_shared_storage(&self.cpu),
        )
    }

    #[inline]
    pub fn memory(&self) -> MemoryView<'_> {
        MemoryView::new(
            &self.bindings.machine_config,
            MemoryAccess::from_shared_storage(&self.guest),
        )
    }

    /// Borrows environment data for one operation. Return owned values or service
    /// handles to use later; the borrow cannot escape this closure.
    ///
    /// ```compile_fail
    /// use relocade_machine::CpuContext;
    /// fn escape(context: &CpuContext<String>) -> &str {
    ///     context.with_state(|state| state.as_str())
    /// }
    /// ```
    pub fn with_state<R>(&self, operation: impl FnOnce(&T) -> R) -> R {
        let core = self.cpu.core();
        let state = core.state.borrow();
        operation(&state)
    }

    pub fn with_state_mut<R>(&self, operation: impl FnOnce(&mut T) -> R) -> R {
        let core = self.cpu.core();
        let mut state = core.state.borrow_mut();
        operation(&mut state)
    }
}
