//! Host and guest entries installed at individual execution addresses.

use super::Hook;
use relocade_compiler::{Func, Program};

/// An independently replaceable execution entry. A [`Hook`] converts directly
/// to a host entry; [`Entry::block`] builds or imports a guest Wasm function.
pub enum Entry {
    Host(Hook),
    Block {
        address: u32,
        build: Box<dyn FnOnce(&mut Program) -> Func>,
    },
}

impl Entry {
    /// Builds or imports a `() -> ExecutionExit` function during installation.
    /// Builders share one compiler Program for the batch. Function signatures
    /// are checked before publication. Use guest dispatch to observe replacement
    /// at another entry. Callers must replace directly linked dependent code too.
    pub fn block(address: u32, build: impl FnOnce(&mut Program) -> Func + 'static) -> Self {
        Self::Block {
            address,
            build: Box::new(build),
        }
    }

    pub(super) fn address(&self) -> u32 {
        match self {
            Self::Host(hook) => hook.address,
            Self::Block { address, .. } => *address,
        }
    }
}

impl From<Hook> for Entry {
    fn from(hook: Hook) -> Self {
        Self::Host(hook)
    }
}
