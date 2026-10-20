use crate::js_error;
use js_sys::{Array, Promise, Uint8Array, WebAssembly};
use relocade_runtime::{
    compilation::{BackgroundCompiler, Compilation},
    CodeSnapshot, CompiledGroup, Result,
};
use std::rc::Rc;
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/src/worker.js")]
extern "C" {
    #[wasm_bindgen(js_name = CompilerWorker)]
    type Worker;
    #[wasm_bindgen(constructor, catch, js_class = CompilerWorker)]
    fn new(bindings_url: &str, wasm_url: &str) -> std::result::Result<Worker, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn submit(this: &Worker, bytes: Uint8Array) -> std::result::Result<JsValue, JsValue>;
    #[wasm_bindgen(method)]
    fn close(this: &Worker);
}

struct WorkerHandle(Worker);

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        self.0.close();
    }
}

/// Discovers, lowers and engine-compiles code in a dedicated browser worker.
/// No shared guest memory or cross-origin isolation is required.
///
/// Supply absolute URLs for this build's `wasm-bindgen --target web` bindings
/// and Wasm binary. The embedding owns those URLs and its worker CSP policy.
/// The bindings must export `relocade_compile_snapshot` (provided by this crate).
/// Their initialization, including any Wasm start function, must run safely in a
/// worker without starting UI code or recursively launching compiler workers.
/// Install with `runtime.set_compiler(Some(Box::new(compiler)))`.
pub struct WebCompiler {
    worker: Rc<WorkerHandle>,
}

impl WebCompiler {
    pub fn new(bindings_url: &str, wasm_url: &str) -> Result<Self> {
        let worker = Worker::new(bindings_url, wasm_url)
            .map_err(|error| js_error("start compiler worker", error))?;
        Ok(Self {
            worker: Rc::new(WorkerHandle(worker)),
        })
    }
}

impl BackgroundCompiler<WebAssembly::Module> for WebCompiler {
    fn submit(
        &mut self,
        snapshot: CodeSnapshot,
    ) -> Result<Option<Compilation<WebAssembly::Module>>> {
        let response = self
            .worker
            .0
            .submit(Uint8Array::from(snapshot.encode()?.as_slice()))
            .map_err(|error| js_error("submit compiler job", error))?;
        if response.is_null() {
            return Ok(None);
        }
        // Attach rejection handling at submission, even if the owner does not
        // poll this job until a later execution slice.
        let response = JsFuture::from(response.unchecked_into::<Promise>());
        let worker = Rc::clone(&self.worker);
        Ok(Some(Box::pin(async move {
            // Retain the worker independently of compiler replacement or a run future.
            let _worker = worker;
            let result = response
                .await
                .map_err(|error| js_error("compile in worker", error))?;
            if result.is_null() {
                return Ok(None);
            }
            let result = result.unchecked_into::<Array>();
            let metadata = Uint8Array::new(&result.get(0)).to_vec();
            let module = result
                .get(1)
                .dyn_into::<WebAssembly::Module>()
                .map_err(|error| js_error("receive compiled module", error))?;
            Ok(Some(CompiledGroup::new(
                snapshot.complete_message(&metadata)?,
                module,
            )))
        })))
    }
}

/// Called by the compiler worker's private Wasm instance, never by guest execution.
#[wasm_bindgen]
pub fn relocade_compile_snapshot(bytes: &[u8]) -> std::result::Result<JsValue, JsValue> {
    let compile = || -> Result<JsValue> {
        let Some(prepared) = CodeSnapshot::compile_message(bytes)? else {
            return Ok(JsValue::NULL);
        };
        let result = Array::new();
        result.push(&Uint8Array::from(prepared.wasm()));
        result.push(&Uint8Array::from(prepared.into_message()?.as_slice()));
        Ok(result.into())
    };
    compile().map_err(|error| JsValue::from_str(&error.to_string()))
}
