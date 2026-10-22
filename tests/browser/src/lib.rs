//! Cross-thread checks shared by the browser suite and native tests.

mod channels;
mod descriptors;
mod memory;
mod modules;
mod ownership;
mod paging;

pub use channels::rust_channels;
pub use descriptors::shared_descriptors;
pub use memory::shared_memory;
pub use modules::compiled_modules;
pub use ownership::shared_address_space;
pub use paging::shared_page_tables;

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

    #[test]
    fn address_spaces_allow_overlapping_access_between_workers() {
        pollster::block_on(super::ownership::shared_address_space()).unwrap();
    }

    #[test]
    fn page_tables_are_shared_between_workers() {
        pollster::block_on(super::paging::shared_page_tables()).unwrap();
    }

    #[test]
    fn descriptor_bytes_and_completion_are_shared_between_workers() {
        pollster::block_on(super::descriptors::shared_descriptors()).unwrap();
    }
}
