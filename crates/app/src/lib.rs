//! Application execution and communication on one worker, independent of a window.
#![forbid(unsafe_code)]
#![allow(async_fn_in_trait)]

mod application;
mod task;
mod worker;

pub use application::{Application, Context};
pub use task::{spawn, Task};

#[cfg(feature = "window")]
pub mod window;
#[cfg(all(feature = "window", target_arch = "wasm32"))]
pub use window::web;
#[cfg(feature = "window")]
pub use window::{open, Control, Input, WindowApplication, WindowOptions};
