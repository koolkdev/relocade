//! Executable dispatch, per-CPU execution and host integration.
//!
//! Wasm modules use the backend's existing engine cache. Bound instances,
//! function tables and continuations belong to each CPU's execution context.

mod backend;
mod bindings;
mod callback;
mod code;
pub(crate) mod dispatch;
mod error;
mod execution;
mod hooks;
pub(crate) mod io;
pub mod real_mode_interrupt;
mod state;
mod storage;
#[cfg(feature = "worker")]
pub mod worker;

pub use backend::RuntimeBackend;
pub use bindings::CpuBindings;
pub use callback::HostCall;
pub use dispatch::DispatchEntry;
pub use error::{Error, Result};
pub use hooks::Hook;
pub(crate) use state::Data as StateData;
pub use state::RuntimeState;
