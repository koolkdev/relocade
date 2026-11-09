use js_sys::{Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Cache, CacheStorage, Response};

pub(super) struct Storage {
    cache: Cache,
}

impl Storage {
    pub(super) async fn open() -> Option<Self> {
        let storage = Reflect::get(&js_sys::global(), &JsValue::from_str("caches"))
            .ok()?
            .dyn_into::<CacheStorage>()
            .ok()?;
        let cache = JsFuture::from(storage.open("relocade-interpreters-v1"))
            .await
            .ok()?
            .dyn_into::<Cache>()
            .ok()?;
        Some(Self { cache })
    }

    pub(super) async fn read(&self, key: &str) -> Option<Vec<u8>> {
        let response = JsFuture::from(self.cache.match_with_str(&url(key)))
            .await
            .ok()?
            .dyn_into::<Response>()
            .ok()?;
        let buffer = JsFuture::from(response.array_buffer().ok()?).await.ok()?;
        Some(Uint8Array::new(&buffer).to_vec())
    }

    pub(super) async fn write(&self, key: &str, bytes: &[u8]) {
        // Copy into an ordinary ArrayBuffer before passing data to the browser.
        let bytes = Uint8Array::from(bytes);
        let Ok(response) = Response::new_with_opt_js_u8_array(Some(&bytes)) else {
            return;
        };
        let _ = JsFuture::from(self.cache.put_with_str(&url(key), &response)).await;
    }
}

fn url(key: &str) -> String {
    // CacheStorage uses URL keys; this request is never sent over the network.
    format!("https://relocade.invalid/{key}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn browser_cache_survives_reopening_and_skips_generation() {
        let storage = Reflect::get(&js_sys::global(), &JsValue::from_str("caches"))
            .unwrap()
            .dyn_into::<CacheStorage>()
            .unwrap();
        let name = format!("relocade-interpreter-test-{}", js_sys::Date::now());
        let open = || async {
            let cache = JsFuture::from(storage.open(&name))
                .await
                .unwrap()
                .dyn_into::<Cache>()
                .unwrap();
            super::super::Cache {
                storage: Some(Storage { cache }),
            }
        };
        let bytes = b"\0asm\x01\0\0\0";
        assert_eq!(
            open()
                .await
                .load_or_generate("test/revision/real16", || async {
                    Ok::<_, ()>(bytes.to_vec())
                })
                .await
                .unwrap(),
            bytes
        );
        assert_eq!(
            open()
                .await
                .load_or_generate("test/revision/real16", || async {
                    Err::<Vec<u8>, _>("cache hit must skip generation")
                })
                .await
                .unwrap(),
            bytes
        );
        JsFuture::from(storage.delete(&name)).await.unwrap();
    }
}
