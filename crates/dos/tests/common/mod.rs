use wasm86_dos::{Dos, DosState};

#[cfg(not(target_arch = "wasm32"))]
pub use wasm86_runtime_wasmtime::WasmtimeBackend as Backend;
#[cfg(target_arch = "wasm32")]
pub use wasm86_runtime_web::WebBackend as Backend;

pub async fn dos() -> Dos<Backend<DosState>> {
    #[cfg(not(target_arch = "wasm32"))]
    let backend = Backend::with_state(DosState::default()).unwrap();
    #[cfg(target_arch = "wasm32")]
    let backend = Backend::with_state(DosState::default());
    Dos::new(backend).await.unwrap()
}
