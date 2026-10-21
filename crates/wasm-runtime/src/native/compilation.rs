use crate::{Error, ModuleCache, Result, WasmCompiler};
use std::sync::OnceLock;
use wasmtime::{Config, Engine, Module};

/// An engine handle that compiles independently of guest stores and host state.
#[derive(Clone)]
pub struct Compiler(Engine);

impl WasmCompiler for Compiler {
    type Module = Module;

    /// Native compilation completes synchronously on the calling thread.
    async fn compile(&self, bytes: &[u8]) -> Result<Module> {
        Module::new(&self.0, bytes).map_err(|error| Error::new("compile Wasm", error))
    }
}

/// Couples shared compiled modules to the engine that can instantiate them.
pub(super) struct Compilation {
    engine: Engine,
    pub(super) modules: ModuleCache<Module>,
}

pub(super) fn shared() -> Result<&'static Compilation> {
    static COMPILATION: OnceLock<std::result::Result<Compilation, String>> = OnceLock::new();
    COMPILATION
        .get_or_init(|| {
            let mut config = Config::new();
            config
                .wasm_multi_memory(true)
                .wasm_tail_call(true)
                .wasm_threads(true)
                .shared_memory(true);
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
                    modules: ModuleCache::default(),
                })
                .map_err(|error| format!("{error:#}"))
        })
        .as_ref()
        .map_err(|error| Error::new("create Wasmtime engine", error))
}

impl Compilation {
    pub(super) fn engine(&self) -> &Engine {
        &self.engine
    }

    pub(super) fn compiler(&self) -> Compiler {
        Compiler(self.engine.clone())
    }
}
