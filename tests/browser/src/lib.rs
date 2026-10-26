//! Cross-thread checks shared by the browser suite and native tests.

mod channels;
mod cpus;
mod memory;
mod modules;

pub use channels::{posted_channels, rust_channels};
pub use cpus::shared_cpu_memory;
pub use memory::{nested_shared_memory, shared_memory};
pub use modules::compiled_modules;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn rust_channels_wake_worker_tasks() {
        pollster::block_on(super::channels::rust_channels()).unwrap();
    }

    #[test]
    fn posted_channels_transfer_endpoints_and_preserve_queued_messages() {
        pollster::block_on(super::channels::posted_channels()).unwrap();
    }

    #[test]
    fn worker_channels_share_memory_and_close() {
        pollster::block_on(super::memory::shared_memory()).unwrap();
    }

    #[test]
    fn cpus_share_memory_across_worker_lifetimes() {
        pollster::block_on(super::cpus::shared_cpu_memory()).unwrap();
    }

    #[test]
    fn memory_outlives_nested_workers() {
        pollster::block_on(super::memory::nested_shared_memory()).unwrap();
    }

    #[test]
    fn compiled_modules_link_to_private_worker_state() {
        pollster::block_on(super::modules::compiled_modules()).unwrap();
    }
}
