//! Browser WebAssembly adapter for the shared x86 runtime.
//!
//! ```
//! # #[cfg(target_arch = "wasm32")]
//! # async fn example() -> wasm86_runtime::Result<()> {
//! use wasm86_runtime::Runtime;
//! use wasm86_runtime_web::WebBackend;
//!
//! let runtime = Runtime::new(WebBackend).await?;
//! # Ok(())
//! # }
//! ```
//!
//! Compile this crate for `wasm32-unknown-unknown` with wasm-bindgen. Preparation
//! awaits JavaScript's WebAssembly promises; guest execution and device callbacks
//! stay synchronous. Run on a worker and yield between bounded `Machine::run_slice`
//! calls to keep the host responsive. Generated Wasm requires multi-memory, tail
//! calls, SIMD and atomics support. Unsupported engines return compilation errors.
#![forbid(unsafe_code)]
#![cfg(target_arch = "wasm32")]

mod imports;
mod instance;
mod memory;

pub use instance::WebInstance;
use js_sys::{Object, Reflect, Uint8Array, WebAssembly};
use wasm86_runtime::{
    backend::{Backend, Imports},
    Devices, Error, Result,
};
use wasm86_x86::CompiledModule;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// Uses the WebAssembly engine of the current JavaScript realm.
#[derive(Clone, Copy, Default)]
pub struct WebBackend;

/// Engine-owned code and its generated entry name.
#[derive(Clone)]
pub struct PreparedModule {
    module: WebAssembly::Module,
    entry: String,
}

impl Backend for WebBackend {
    type Module = PreparedModule;
    type Instance<D: Devices + 'static> = WebInstance<D>;

    async fn compile(&self, module: &CompiledModule) -> Result<PreparedModule> {
        // Copy into a JS-owned buffer so compilation cannot retain a view into
        // Rust's growable linear memory across the await.
        let bytes = Uint8Array::from(module.bytes.as_slice());
        let module_value = JsFuture::from(WebAssembly::compile(&bytes))
            .await
            .map_err(|error| js_error("compile Wasm", error))?;
        Ok(PreparedModule {
            module: module_value
                .dyn_into()
                .map_err(|error| js_error("read compiled module", error))?,
            entry: module.entry.clone(),
        })
    }

    async fn instantiate<D: Devices + 'static>(
        &self,
        module: &PreparedModule,
        dispatcher: &PreparedModule,
        imports: Imports<D>,
    ) -> Result<WebInstance<D>> {
        WebInstance::new(module, dispatcher, imports).await
    }
}

fn js_error(operation: &'static str, error: JsValue) -> Error {
    let message = if let Some(error) = error.dyn_ref::<js_sys::Error>() {
        String::from(error.to_string())
    } else {
        error.as_string().unwrap_or_else(|| format!("{error:?}"))
    };
    Error::backend(operation, message)
}

fn set_property(object: &Object, name: &str, value: &JsValue) {
    Reflect::set(object, &name.into(), value).expect("owned object accepts properties");
}
