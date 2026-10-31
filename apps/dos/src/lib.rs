//! Rust DOS command console, sharing the guest's keyboard and VGA state.
#![forbid(unsafe_code)]

mod console;
pub use console::run_console;
