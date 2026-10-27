//! Worker-local application execution and rendering, independent of windows.
#![deny(unsafe_code)]

#[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
compile_error!("relocade-app requires a threaded browser build");

mod application;
mod task;

pub use application::{spawn, Application, Context};
pub use relocade_renderer::{Graphics, Renderer, Texture};
pub use task::{Exit, Task};
