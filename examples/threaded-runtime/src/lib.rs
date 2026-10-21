//! Native and browser checks for a shared Rust heap with worker-local async tasks.

use futures_channel::{mpsc, oneshot};
use futures_util::StreamExt;
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};
use web_thread::Thread;

mod compiler;
mod cpu;

#[cfg(not(target_arch = "wasm32"))]
use relocade_runtime_wasmtime::WasmtimeBackend as Backend;
#[cfg(target_arch = "wasm32")]
use relocade_runtime_web::WebBackend as Backend;

fn backend<T: 'static>(state: T) -> Result<Backend<T>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    return Backend::with_state(state).map_err(|error| error.to_string());
    #[cfg(target_arch = "wasm32")]
    Ok(Backend::with_state(state))
}

thread_local! {
    static LOCAL: Cell<bool> = const { Cell::new(false) };
}

// This message crosses threads as an ordinary Rust value. No serialization or
// JavaScript representation is needed, including for its shared counter.
struct Command {
    bytes: Vec<u8>,
    counter: Arc<AtomicU32>,
}

async fn check_threads() -> Result<(), String> {
    LOCAL.set(true);
    let thread = Thread::new();
    let (commands, mut incoming) = mpsc::unbounded::<Command>();
    let (ready, started) = oneshot::channel();
    let result = thread.run_send((), move |()| async move {
        assert!(
            !LOCAL.get(),
            "each worker needs its own thread-local storage"
        );
        // Created on the worker, this state and the future holding it need not
        // implement Send. Only the startup closure crosses threads.
        let local = Rc::new(Cell::new(40));
        ready.send(()).unwrap();
        let command = incoming.next().await.unwrap();
        let address = command.bytes.as_ptr() as usize;
        local.set(local.get() + u32::from(command.bytes[0]));
        command.counter.store(local.get(), Ordering::Release);
        assert!(incoming.next().await.is_none());
        (command.bytes, address)
    });

    started.await.map_err(|error| error.to_string())?;
    let counter = Arc::new(AtomicU32::new(0));
    let bytes = vec![2, 3, 4];
    let address = bytes.as_ptr() as usize;
    commands
        .unbounded_send(Command {
            bytes,
            counter: Arc::clone(&counter),
        })
        .map_err(|error| error.to_string())?;
    drop(commands);
    let (bytes, worker_address) = result.await.map_err(|error| error.to_string())?;
    assert_eq!(bytes, [2, 3, 4]);
    assert_eq!(
        address, worker_address,
        "the worker receives the same allocation"
    );
    assert_eq!(address, bytes.as_ptr() as usize);
    assert_eq!(counter.load(Ordering::Acquire), 42);
    assert!(LOCAL.get());
    // Finish the persistent task before dropping its thread owner.
    drop(thread);
    Ok(())
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn run_checks() -> Result<(), String> {
    check_threads().await?;
    compiler::check().await?;
    cpu::check().await
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn shared_rust_messages_wake_a_worker_local_future() {
        pollster::block_on(super::check_threads()).unwrap();
    }

    #[test]
    fn compiled_modules_reach_another_execution_owner() {
        pollster::block_on(super::compiler::check()).unwrap();
    }

    #[test]
    fn cpu_task_receives_commands_and_awaits_deferred_input() {
        pollster::block_on(super::cpu::check()).unwrap();
    }
}
