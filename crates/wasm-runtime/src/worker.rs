//! Rust workers that run local async tasks.
//!
//! [`Thread::run`] and [`channel`] use platform transport for [`Post`] values,
//! including shared Wasm memory and compiled modules. Ordinary Rust `Send`
//! values can also travel through captured Rust channels or `run_send` results.
//! Finish accepted tasks before dropping their `Thread`.

#[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
compile_error!("worker requires a threaded browser build; see tests/browser/README.md");

pub use web_thread::{Error, Post, SendTask, Task, Thread};

#[cfg_attr(not(target_arch = "wasm32"), path = "worker/native.rs")]
#[cfg_attr(target_arch = "wasm32", path = "worker/web.rs")]
mod channel;
pub use channel::{channel, Receiver, Sender};

/// Adapts serializable values without transferable objects to [`Post`].
/// Use an object's own Post implementation when it transfers ownership, as
/// channel endpoints do. Shared memory is cloned by the browser transport.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Message<T>(pub T);

#[cfg(target_arch = "wasm32")]
impl<T: serde::Serialize + serde::de::DeserializeOwned> Post for Message<T> {}
