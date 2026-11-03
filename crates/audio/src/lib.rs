//! CPAL output with a bounded Rust channel of interleaved sample buffers.
#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod output;
pub use cpal;
pub use output::Output;

#[cfg(all(test, target_arch = "wasm32"))]
#[path = "tests/browser.rs"]
mod browser_tests;
