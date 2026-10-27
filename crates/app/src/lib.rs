//! Application execution, rendering and a desktop/browser window host.
#![deny(unsafe_code)]

#[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
compile_error!("relocade-app requires a threaded browser build; see crates/app/README.md");

mod application;
mod control;
mod display;
#[cfg_attr(not(target_arch = "wasm32"), path = "platform/native.rs")]
#[cfg_attr(target_arch = "wasm32", path = "platform/web.rs")]
mod platform;
mod session;
mod task;
mod window;

#[cfg(target_arch = "wasm32")]
#[path = "launch/web.rs"]
pub mod web;

pub use application::{spawn, Application, Context};
pub use control::{Control, StopHandle};
pub use relocade_renderer::{Graphics, Renderer, Texture};
pub use task::{Exit, Task};
pub use window::{run, OnCompletion, WindowOptions};
