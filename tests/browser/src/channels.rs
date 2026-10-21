use futures_channel::{mpsc, oneshot};
use futures_util::{future::poll_fn, StreamExt};
use relocade_machine::worker::Thread;
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

thread_local! {
    static CALLER: Cell<bool> = const { Cell::new(false) };
}

// Ordinary Rust commands use shared host memory, without Serde or Message.
struct Command {
    bytes: Vec<u8>,
    counter: Arc<AtomicU32>,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn rust_channels() -> Result<(), String> {
    CALLER.set(true);
    let thread = Thread::new();
    let (commands, mut incoming) = mpsc::unbounded::<Command>();
    let (waiting, started) = oneshot::channel();
    let task = thread.run_send((), move |()| async move {
        assert!(!CALLER.get(), "workers must have independent TLS");
        let local = Rc::new(Cell::new(40));
        let mut waiting = Some(waiting);
        let command = poll_fn(|context| {
            let poll = incoming.poll_next_unpin(context);
            if poll.is_pending() {
                // Announce only after registering the channel's waker. The
                // caller then sends a command to this genuinely pending task.
                if let Some(waiting) = waiting.take() {
                    waiting.send(()).unwrap();
                }
            }
            poll
        })
        .await
        .unwrap();
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
    let (bytes, worker_address) = task.await.map_err(|error| error.to_string())?;
    assert_eq!(bytes, [2, 3, 4]);
    assert_eq!(worker_address, address);
    assert_eq!(bytes.as_ptr() as usize, address);
    assert_eq!(counter.load(Ordering::Acquire), 42);
    assert!(CALLER.get());
    Ok(())
}
