//! Cross-thread checks shared by the browser suite and native tests.

mod channels;
mod cpus;
mod memory;
mod modules;
mod vga;

pub use channels::rust_channels;
pub use cpus::cpu_worker;
pub use memory::shared_memory;
pub use modules::compiled_modules;
pub use vga::vga_workers;

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
    fn cpu_workers_await_rust_input_and_share_guest_memory() {
        pollster::block_on(super::cpus::cpu_worker()).unwrap();
    }

    #[test]
    fn vga_display_progresses_while_cpu_awaits_input() {
        pollster::block_on(super::vga::vga_workers()).unwrap();
    }
}
