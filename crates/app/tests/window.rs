#[cfg(target_os = "linux")]
#[path = "window/native.rs"]
mod native;

#[cfg(target_arch = "wasm32")]
#[path = "window/web.rs"]
mod web;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
