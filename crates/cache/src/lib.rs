//! Persistent generated artifacts shared by native and browser consumers.
//!
//! Callers define artifact formats and keys, including their namespace, format
//! version and every input that affects generation. This crate stores opaque
//! bytes without depending on a compiler, execution profile or Wasm engine.

use std::future::Future;

#[cfg_attr(target_arch = "wasm32", path = "web.rs")]
#[cfg_attr(not(target_arch = "wasm32"), path = "native.rs")]
mod storage;

/// Best-effort storage for generated artifacts.
///
/// Native storage uses the user's cache directory and verifies content integrity.
/// Browsers use origin-scoped CacheStorage. Entries are disposable; old versions
/// are not automatically evicted. This cache does not coordinate generation:
/// concurrent misses may generate the same artifact independently.
pub struct ArtifactCache {
    storage: Option<storage::Storage>,
}

impl ArtifactCache {
    /// Opens the application's shared artifact storage.
    /// Unavailable storage leaves generation usable without persistence.
    pub async fn open() -> Self {
        Self {
            storage: storage::Storage::open().await,
        }
    }

    /// Loads the artifact, invoking `generate` only on a cache miss.
    ///
    /// Keys are opaque, case-sensitive strings shared by all consumers. Include
    /// a consumer namespace, artifact version and all generation inputs in `key`.
    /// The caller defines the byte format, including any metadata needed on reuse.
    /// Cache I/O is best effort; generation errors are returned unchanged and
    /// failed generation is never cached.
    pub async fn load_or_generate<E, F>(
        &self,
        key: &str,
        generate: impl FnOnce() -> F,
    ) -> Result<Vec<u8>, E>
    where
        F: Future<Output = Result<Vec<u8>, E>>,
    {
        if let Some(storage) = &self.storage {
            if let Some(bytes) = storage.read(key).await {
                return Ok(bytes);
            }
        }
        let bytes = generate().await?;
        if let Some(storage) = &self.storage {
            storage.write(key, &bytes).await;
        }
        Ok(bytes)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
