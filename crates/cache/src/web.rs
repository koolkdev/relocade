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
        let cache = JsFuture::from(storage.open("relocade-artifacts-v1"))
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
    // An encoded query preserves opaque keys without path or fragment normalization.
    format!(
        "https://relocade.invalid/artifact?key={}",
        js_sys::encode_uri_component(key)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn browser_cache_survives_reopening_and_preserves_opaque_keys() {
        let storage = Reflect::get(&js_sys::global(), &JsValue::from_str("caches"))
            .unwrap()
            .dyn_into::<CacheStorage>()
            .unwrap();
        let name = format!("relocade-artifact-test-{}", js_sys::Date::now());
        let open = || async {
            let cache = JsFuture::from(storage.open(&name))
                .await
                .unwrap()
                .dyn_into::<Cache>()
                .unwrap();
            crate::ArtifactCache {
                storage: Some(Storage { cache }),
            }
        };
        let keys = [
            "", ".", "..", "a/../b", "b", "b#one", "b#two", "b?one", "b?two", "a\\b", "a/b", "%2E",
            "%2F", " ", "%20", "B", "λ",
        ];
        let cache = open().await;
        for (index, key) in keys.iter().enumerate() {
            let bytes = vec![index as u8];
            assert_eq!(
                cache
                    .load_or_generate(key, || async { Ok::<_, ()>(bytes.clone()) })
                    .await
                    .unwrap(),
                bytes,
                "distinct key must generate: {key:?}"
            );
        }
        let reopened = open().await;
        for (index, key) in keys.iter().enumerate() {
            assert_eq!(
                reopened
                    .load_or_generate(key, || async {
                        Err::<Vec<u8>, _>("cache hit must skip generation")
                    })
                    .await
                    .unwrap(),
                [index as u8],
                "reopening must preserve key: {key:?}"
            );
        }
        JsFuture::from(storage.delete(&name)).await.unwrap();
    }
}
