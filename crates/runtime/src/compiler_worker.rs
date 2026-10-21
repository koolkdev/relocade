//! Shared compilation handler for `web-thread` tasks.
//!
//! The embedding owns the thread and calls [`compile`] inside `Thread::run`.
//! The library owns task delivery and completion; execution owners retain their
//! instances, imports, CPU and memory.

#[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
compile_error!(
    "compiler-worker requires a threaded browser build; see runtime/docs/compiler-workers.md"
);

use crate::{wasm::WasmBackend, Result};
pub use web_thread::Thread;

/// Compiled code carried by the thread library's existing `Post` transport.
/// Native modules move as Rust values; browser modules are structured-cloned.
#[cfg_attr(target_arch = "wasm32", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(target_arch = "wasm32", serde(bound = "M: wasm_bindgen::JsCast"))]
pub struct CompiledModule<M> {
    #[cfg_attr(target_arch = "wasm32", serde(with = "serde_wasm_bindgen::preserve"))]
    pub module: M,
}

#[cfg(target_arch = "wasm32")]
impl<M: wasm_bindgen::JsCast> web_thread::Post for CompiledModule<M> {}

/// Lowers owned input and compiles the resulting Wasm inside a worker task.
///
/// Capture requests in the task's Send closure. The backend is created on that
/// thread and need not be Send. The Wasmtime backend already shares its engine
/// with execution owners. Errors fail this task without stopping the thread.
/// Diagnostics cross through the library as strings, like its other Post values.
pub async fn compile<B: WasmBackend>(
    create_backend: impl FnOnce() -> Result<B>,
    lower: impl FnOnce() -> Result<Vec<u8>>,
) -> std::result::Result<CompiledModule<B::Module>, String> {
    let compile = async {
        let bytes = lower()?;
        let module = create_backend()?.compile(&bytes).await?;
        Ok::<_, crate::Error>(CompiledModule { module })
    };
    compile.await.map_err(|error| error.to_string())
}
