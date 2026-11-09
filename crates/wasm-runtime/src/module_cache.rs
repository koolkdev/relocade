use futures_util::lock::Mutex;
use std::{collections::HashMap, future::Future, sync::Arc};

type ModuleSlot<M> = Arc<Mutex<Option<M>>>;

/// Compiled modules retained by their caller's artifact identity.
///
/// The owner establishes engine compatibility. Keys must identify every input
/// to generation, including the generator revision and configuration. This lets
/// callers look up a module before generating or reading its Wasm bytes.
/// Entries live as long as the cache.
pub struct ModuleCache<M> {
    modules: Mutex<HashMap<Vec<u8>, ModuleSlot<M>>>,
}

impl<M> Default for ModuleCache<M> {
    fn default() -> Self {
        Self {
            modules: Mutex::new(HashMap::new()),
        }
    }
}

impl<M: Clone> ModuleCache<M> {
    /// Reuses this artifact's module, generating and compiling it on a miss.
    ///
    /// `compile` must use an engine compatible with every user of this cache.
    /// Concurrent requests for one key wait for its compilation; different keys
    /// can compile independently. Failed or cancelled requests allow retry.
    pub async fn get_or_compile<E, F>(
        &self,
        key: &[u8],
        compile: impl FnOnce() -> F,
    ) -> Result<M, E>
    where
        F: Future<Output = Result<M, E>>,
    {
        let slot = {
            let mut modules = self.modules.lock().await;
            modules.entry(key.to_vec()).or_default().clone()
        };
        let mut module = slot.lock().await;
        if let Some(module) = module.as_ref() {
            return Ok(module.clone());
        }
        let compiled = compile().await?;
        *module = Some(compiled.clone());
        Ok(compiled)
    }
}

#[cfg(test)]
mod tests;
