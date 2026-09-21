use std::{
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
};

use serde::{de::DeserializeOwned, Serialize};

#[cfg(test)]
#[path = "module_tests.rs"]
mod tests;

/// Immutable Wasm bytes with lazy compilation in each test engine.
/// Execution hosts still create fresh stores, imports and instances per case.
pub struct Module {
    bytes: Vec<u8>,
    compiled: OnceLock<wasmtime::Module>,
    pub(super) id: usize,
}

impl Module {
    pub fn new(bytes: &[u8]) -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        Self {
            bytes: bytes.into(),
            compiled: OnceLock::new(),
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn wasmtime(&self) -> &wasmtime::Module {
        self.compiled.get_or_init(|| {
            wasmtime::Module::new(crate::engine(), &self.bytes).expect("compile test module")
        })
    }

    /// Call an adapter's default export `(module, input)` under TurboFan.
    /// Compilation is shared; the adapter owns fresh execution state per call.
    pub fn run_v8<T: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        adapter: &Path,
        input: &T,
    ) -> R {
        crate::v8::run(adapter, self, input)
    }
}
