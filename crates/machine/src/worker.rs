//! Thread tasks and transport for native values and browser objects.

#[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
compile_error!("worker requires a threaded browser build; see machine/docs/workers.md");

pub use web_thread::Thread;

/// Carries a payload through the thread library's existing `Post` transport.
///
/// Native payloads move as Send Rust values. Browser payloads use Serde and
/// structured cloning; backend resource types preserve their JavaScript handles.
/// Shared memories keep the same backing bytes. This wrapper adds no transfer
/// list for objects that require one and provides no memory synchronization.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Message<T>(pub T);

#[cfg(target_arch = "wasm32")]
impl<T: serde::Serialize + serde::de::DeserializeOwned> web_thread::Post for Message<T> {}

use crate::{Machine, Memory};
use relocade_wasm_runtime::SharedMemory;
use std::{future::Future, sync::Arc};

impl Machine<SharedMemory> {
    /// Runs a task with this same machine on a worker. Shared Wasm allocations
    /// travel through structured cloning; CPU membership and runtime code remain
    /// shared Rust owners. Construct the CPU's backend inside the task.
    pub fn on_worker<F: Future<Output: Send> + 'static>(
        &self,
        thread: &Thread,
        task: impl FnOnce(Self) -> F + Send + 'static,
    ) -> web_thread::SendTask<F::Output> {
        let config = Arc::clone(&self.memory.config);
        let shared = Arc::clone(&self.shared);
        thread.run_send(
            Message(self.memory.backing().clone()),
            move |Message(backing)| {
                let memory =
                    Memory::with_backing(config, backing).expect("transported machine allocations");
                task(Self { memory, shared })
            },
        )
    }
}
