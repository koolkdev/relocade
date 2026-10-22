use crate::Result;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use relocade_x86::{Compiler, ExecutionTarget};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, TryLockError},
};

type Dispatchers = HashMap<ExecutionTarget, Arc<[u8]>>;

/// Code shared by a machine's CPUs. Engine objects remain in the backend cache,
/// because browser modules cannot be used directly from another worker.
#[derive(Default)]
pub(crate) struct Runtime {
    dispatchers: Mutex<Dispatchers>,
}

impl Runtime {
    fn dispatcher_source(&self, target: ExecutionTarget) -> Result<Arc<[u8]>> {
        // Browser main threads cannot block on another worker's Rust mutex.
        // Contention may duplicate generation, but never waits for another worker.
        if let Some(dispatchers) = self.cached_dispatchers() {
            if let Some(bytes) = dispatchers.get(&target) {
                return Ok(Arc::clone(bytes));
            }
        }
        let compiler = Compiler::new(target.profile).with_cpu_model(target.cpu_model);
        let bytes: Arc<[u8]> = compiler.compile_dispatcher()?.bytes.into();
        if let Some(mut dispatchers) = self.cached_dispatchers() {
            dispatchers
                .entry(target)
                .or_insert_with(|| Arc::clone(&bytes));
        }
        Ok(bytes)
    }

    fn cached_dispatchers(&self) -> Option<MutexGuard<'_, Dispatchers>> {
        match self.dispatchers.try_lock() {
            Ok(cache) => Some(cache),
            Err(TryLockError::WouldBlock) => None,
            Err(TryLockError::Poisoned(_)) => panic!("dispatcher cache"),
        }
    }

    pub async fn dispatcher<B: WasmBackend>(
        &self,
        backend: &B,
        target: ExecutionTarget,
    ) -> Result<B::Module> {
        let bytes = self.dispatcher_source(target)?;
        Ok(backend.compiler().compile(&bytes).await?)
    }
}

pub(super) async fn interpreter<B: WasmBackend>(
    backend: &B,
    target: ExecutionTarget,
) -> Result<B::Module> {
    let bytes = relocade_interpreter::module(target.profile);
    let compiler = backend.compiler();
    Ok(if let Some(cache) = backend.module_cache() {
        cache
            .get_or_compile(bytes, |bytes| compiler.compile(bytes))
            .await?
    } else {
        compiler.compile(bytes).await?
    })
}
