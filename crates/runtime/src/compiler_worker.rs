//! Shared compilation handler for `web-thread` tasks.
//!
//! The embedding owns the thread and calls [`compile`] inside
//! [`Thread::run`](crate::worker::Thread::run).
//! The library owns task delivery and completion; execution owners retain their
//! instances, imports, CPU and memory.

use crate::{wasm::WasmBackend, worker::StructuredClone, Result};

/// Lowers owned input and compiles the resulting Wasm inside a worker task.
///
/// Capture requests in the task's Send closure. The backend is created on that
/// thread and need not be Send. The Wasmtime backend already shares its engine
/// with execution owners. Errors fail this task without stopping the thread.
/// Diagnostics cross through the library as strings, like its other Post values.
pub async fn compile<B: WasmBackend>(
    create_backend: impl FnOnce() -> Result<B>,
    lower: impl FnOnce() -> Result<Vec<u8>>,
) -> std::result::Result<StructuredClone<B::Module>, String> {
    let compile = async {
        let bytes = lower()?;
        let module = create_backend()?.compile(&bytes).await?;
        Ok::<_, crate::Error>(StructuredClone(module))
    };
    compile.await.map_err(|error| error.to_string())
}
