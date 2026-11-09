use std::future::Future;

#[cfg_attr(target_arch = "wasm32", path = "cache/web.rs")]
#[cfg_attr(not(target_arch = "wasm32"), path = "cache/native.rs")]
mod storage;

pub(super) struct Cache {
    storage: Option<storage::Storage>,
}

impl Cache {
    pub(super) async fn open() -> Self {
        Self {
            storage: storage::Storage::open().await,
        }
    }

    pub(super) async fn load_or_generate<E, F>(
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
