//! Host-facing peripheral contracts shared by applications and device adapters.
//! Device implementations, execution and application ownership stay with their
//! respective crates. Enable `display` for the graphics-side display interface.
#![forbid(unsafe_code)]

#[cfg(feature = "display")]
pub mod display;

#[cfg(feature = "keyboard")]
pub mod keyboard;
