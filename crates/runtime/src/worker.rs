//! Thread tasks and transport for native values and browser objects.

#[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
compile_error!("worker requires a threaded browser build; see runtime/docs/compiler-workers.md");

pub use web_thread::Thread;

/// Carries a value through the thread library's existing `Post` transport.
///
/// Native values move through Rust channels and must be Send. On browsers, `T`
/// must be a JavaScript object supported by structured cloning, such as a
/// `WebAssembly.Module` or shared `WebAssembly.Memory`. Serde preserves the
/// object handle; the library's `postMessage` call performs the structured clone.
/// This wrapper does not add a transfer list for objects that require one.
#[cfg_attr(target_arch = "wasm32", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(target_arch = "wasm32", serde(bound = "T: wasm_bindgen::JsCast"))]
pub struct StructuredClone<T>(
    #[cfg_attr(target_arch = "wasm32", serde(with = "serde_wasm_bindgen::preserve"))] pub T,
);

#[cfg(target_arch = "wasm32")]
impl<T: wasm_bindgen::JsCast> web_thread::Post for StructuredClone<T> {}
