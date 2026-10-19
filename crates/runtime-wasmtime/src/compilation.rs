use relocade_runtime::{Error, Result};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};
use wasmtime::{Config, Engine, Module};

/// Compiled code is process-wide; stores and all guest state remain private.
pub(super) struct Compilation {
    engine: Engine,
    modules: Mutex<HashMap<&'static [u8], Module>>,
}

pub(super) fn shared() -> Result<&'static Compilation> {
    static COMPILATION: OnceLock<std::result::Result<Compilation, String>> = OnceLock::new();
    COMPILATION
        .get_or_init(|| {
            let mut config = Config::new();
            config
                .wasm_multi_memory(true)
                .wasm_tail_call(true)
                .wasm_threads(true);
            match wasmtime::Cache::from_file(None) {
                Ok(cache) => {
                    config.cache(Some(cache));
                }
                Err(error) => {
                    eprintln!("Wasmtime cache unavailable; continuing uncached: {error}");
                }
            }
            Engine::new(&config)
                .map(|engine| Compilation {
                    engine,
                    modules: Mutex::new(HashMap::new()),
                })
                .map_err(|error| format!("{error:#}"))
        })
        .as_ref()
        .map_err(|error| Error::wasm("create Wasmtime engine", error))
}

impl Compilation {
    pub(super) fn engine(&self) -> &Engine {
        &self.engine
    }

    pub(super) fn compile_shared(&self, bytes: &'static [u8]) -> Result<Module> {
        let mut modules = self
            .modules
            .lock()
            .map_err(|error| Error::wasm("lock shared Wasm modules", error))?;
        if let Some(module) = modules.get(bytes) {
            return Ok(module.clone());
        }
        // Keep the lock during compilation so concurrent runtime creation does
        // not compile the same embedded asset more than once.
        let module = Module::new(&self.engine, bytes)
            .map_err(|error| Error::wasm("compile shared Wasm", error))?;
        modules.insert(bytes, module.clone());
        Ok(module)
    }
}
