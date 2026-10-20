use crate::Result;
use futures_util::lock::Mutex;
use std::{collections::HashMap, future::Future, sync::Arc};

type ModuleSlot<M> = Arc<Mutex<Option<M>>>;

/// Compiled modules retained for a fixed set of embedded assets.
///
/// The owner establishes engine compatibility; callers choose which assets to
/// retain. Entries live as long as the cache. Asset identity avoids hashing the
/// entire Wasm input on each lookup.
pub struct ModuleCache<M> {
    modules: Mutex<HashMap<AssetId, ModuleSlot<M>>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct AssetId {
    address: usize,
    length: usize,
}

impl<M> Default for ModuleCache<M> {
    fn default() -> Self {
        Self {
            modules: Mutex::new(HashMap::new()),
        }
    }
}

impl<M: Clone> ModuleCache<M> {
    /// Reuses this asset's module, compiling it on a miss.
    ///
    /// Keys use the static slice's address and length. Equal bytes in separate
    /// allocations are distinct assets.
    ///
    /// `compile` must use an engine compatible with every user of this cache.
    /// Concurrent requests for one asset wait for its compilation; different
    /// assets can compile independently. Failed or cancelled requests allow retry.
    pub async fn get_or_compile<F>(
        &self,
        bytes: &'static [u8],
        compile: impl FnOnce(&'static [u8]) -> F,
    ) -> Result<M>
    where
        F: Future<Output = Result<M>>,
    {
        let slot = {
            let mut modules = self.modules.lock().await;
            modules
                .entry(AssetId {
                    address: bytes.as_ptr() as usize,
                    length: bytes.len(),
                })
                .or_default()
                .clone()
        };
        let mut module = slot.lock().await;
        if let Some(module) = module.as_ref() {
            return Ok(module.clone());
        }
        let compiled = compile(bytes).await?;
        *module = Some(compiled.clone());
        Ok(compiled)
    }
}

#[cfg(test)]
mod tests;
