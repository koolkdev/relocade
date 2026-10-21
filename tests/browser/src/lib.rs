//! Cross-thread checks shared by the browser suite and native tests.

mod channels;
mod memory;
mod modules;

pub use channels::rust_channels;
pub use memory::shared_memory;
pub use modules::compiled_modules;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
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
