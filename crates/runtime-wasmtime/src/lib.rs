//! Wasmtime adapter for the shared x86 runtime.
//!
//! ```
//! use wasm86_runtime::Runtime;
//! use wasm86_runtime_wasmtime::WasmtimeBackend;
//!
//! # async fn example() -> wasm86_runtime::Result<()> {
//! let runtime = Runtime::new(WasmtimeBackend::new()?).await?;
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]
#![cfg(not(target_arch = "wasm32"))]

mod instance;
mod memory;

pub use instance::WasmtimeInstance;
use wasm86_runtime::{
    backend::{Backend, Imports},
    Devices, Error, Result,
};
use wasm86_x86::CompiledModule;
use wasmtime::{Config, Engine, Module};

/// Native Wasm engine; cloning shares Wasmtime's engine resources.
#[derive(Clone)]
pub struct WasmtimeBackend {
    engine: Engine,
}

/// Engine-owned code and its generated entry name.
#[derive(Clone)]
pub struct PreparedModule {
    module: Module,
    entry: String,
}

impl WasmtimeBackend {
    pub fn new() -> Result<Self> {
        let mut config = Config::new();
        config
            .wasm_multi_memory(true)
            .wasm_tail_call(true)
            .wasm_threads(true);
        let engine = Engine::new(&config)
            .map_err(|error| Error::backend("create Wasmtime engine", error))?;
        Ok(Self { engine })
    }
}

impl Backend for WasmtimeBackend {
    type Module = PreparedModule;
    type Instance<D: Devices + 'static> = WasmtimeInstance<D>;

    /// Wasmtime compilation completes synchronously on the calling thread.
    async fn compile(&self, module: &CompiledModule) -> Result<PreparedModule> {
        let compiled = Module::new(&self.engine, &module.bytes)
            .map_err(|error| Error::backend("compile Wasm", error))?;
        Ok(PreparedModule {
            module: compiled,
            entry: module.entry.clone(),
        })
    }

    async fn instantiate<D: Devices + 'static>(
        &self,
        module: &PreparedModule,
        imports: Imports<D>,
    ) -> Result<WasmtimeInstance<D>> {
        WasmtimeInstance::new(&self.engine, module, imports)
            .map_err(|error| Error::backend("instantiate Wasm", error))
    }
}
