//! Application integration contracts for execution and graphics adapters.
//! Keyboard events use winit values; display adapters share the application's
//! wgpu resources. Window, worker and guest ownership remain with their callers.
#![forbid(unsafe_code)]

#[cfg(feature = "display")]
pub mod display;

#[cfg(feature = "keyboard")]
pub mod keyboard;
