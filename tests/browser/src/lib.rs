//! Cross-thread checks shared by the browser suite and native tests.

mod channels;
mod compilation;
mod memory;
mod modules;

pub use channels::rust_channels;
pub use compilation::worker_compilation;
pub use memory::shared_memory;
pub use modules::compiled_modules;

#[cfg(not(target_arch = "wasm32"))]
use relocade_runtime_wasmtime::WasmtimeBackend as Backend;
#[cfg(target_arch = "wasm32")]
use relocade_runtime_web::WebBackend as Backend;

fn backend<T: 'static>(state: T) -> relocade_runtime::Result<Backend<T>> {
    #[cfg(not(target_arch = "wasm32"))]
    return Backend::with_state(state);
    #[cfg(target_arch = "wasm32")]
    Ok(Backend::with_state(state))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn backends_compile_on_a_reusable_worker() {
        pollster::block_on(super::compilation::worker_compilation()).unwrap();
    }

    #[test]
    fn rust_channels_wake_worker_tasks() {
        pollster::block_on(super::channels::rust_channels()).unwrap();
    }

    #[test]
    fn messages_preserve_shared_memory() {
        pollster::block_on(super::memory::shared_memory()).unwrap();
    }

    #[test]
    fn compiled_modules_link_to_private_worker_state() {
        pollster::block_on(super::modules::compiled_modules()).unwrap();
    }
}
