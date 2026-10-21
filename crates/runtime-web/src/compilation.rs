use crate::js_error;
use js_sys::{Uint8Array, WebAssembly};
use relocade_runtime::{
    wasm::{CompilerWorker, ModuleCache, WasmCompiler},
    Result,
};
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// Compiled code whose JavaScript handle can be posted to another worker.
/// Equality compares handle identity, including across clones of this value.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Module(#[serde(with = "serde_wasm_bindgen::preserve")] pub(super) WebAssembly::Module);

pub(super) struct Compilation {
    pub(super) modules: ModuleCache<Module>,
    pub(super) worker: CompilerWorker<Compiler>,
}

pub(super) fn shared() -> Rc<Compilation> {
    thread_local! {
        static COMPILATION: Rc<Compilation> = Rc::new(Compilation {
            modules: ModuleCache::default(),
            worker: CompilerWorker::new(Compiler),
        });
    }
    COMPILATION.with(Rc::clone)
}

#[derive(Clone)]
pub(super) struct Compiler;

impl WasmCompiler for Compiler {
    type Module = Module;

    async fn compile(&self, bytes: &[u8]) -> Result<Module> {
        let bytes = Uint8Array::from(bytes);
        JsFuture::from(WebAssembly::compile(&bytes))
            .await
            .map(|module| Module(module.unchecked_into()))
            .map_err(|error| js_error("compile Wasm", error))
    }
}
