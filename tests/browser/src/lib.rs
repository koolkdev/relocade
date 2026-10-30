//! Cross-thread checks shared by the browser suite and native tests.

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn initialize() {
    console_error_panic_hook::set_once();
}

#[path = "../../../crates/vga/tests/support/mod.rs"]
mod vga_support;
use vga_support::{fixture as vga_fixture, software};

mod channels;
mod clock;
mod cpus;
mod dos;
mod graphics;
mod memory;
mod modules;
mod runner;
mod vga;

pub use channels::rust_channels;
pub use clock::timer_workers;
pub use cpus::cpu_worker;
pub use dos::dos_workers;
pub use graphics::{gpu::gpu_workers, graphics_workers};
pub use memory::shared_memory;
pub use modules::compiled_modules;
pub use runner::runner_workers;
pub use vga::vga_workers;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn dos_sessions_keep_captures_across_input_launches_and_host_redraws() {
        pollster::block_on(super::dos::dos_workers()).unwrap();
    }

    #[test]
    fn gpu_rendering_uses_explicit_worker_demand_without_a_surface() {
        pollster::block_on(super::gpu_workers()).unwrap();
    }

    #[test]
    fn graphics_demand_does_not_stall_execution_or_queue_frames() {
        pollster::block_on(super::graphics::graphics_workers()).unwrap();
    }

    #[test]
    fn runner_stops_busy_or_waiting_cpus_after_host_timer_delivery() {
        pollster::block_on(super::runner::runner_workers()).unwrap();
    }

    #[test]
    fn host_timers_wake_a_continuously_runnable_worker() {
        pollster::block_on(super::clock::timer_workers()).unwrap();
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

    #[test]
    fn cpu_workers_await_rust_input_and_share_guest_memory() {
        pollster::block_on(super::cpus::cpu_worker()).unwrap();
    }

    #[test]
    fn vga_display_progresses_while_cpu_awaits_input() {
        pollster::block_on(super::vga::vga_workers()).unwrap();
    }
}
