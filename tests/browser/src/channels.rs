use futures_channel::{mpsc, oneshot};
use futures_util::{future::poll_fn, StreamExt};
use relocade_machine::worker::{channel, Sender, Thread};
use std::{
    cell::Cell,
    future::Future,
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
    assert_eq!(
        thread
            .run((), |()| async { 37_u32 })
            .await
            .map_err(|error| error.to_string())?,
        37
    );
    Ok(())
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn posted_channels() -> Result<(), String> {
    let thread = Thread::new();
    let (requests, incoming) = channel::<Sender<u32>>().map_err(|error| error.to_string())?;
    let (replies, mut received) = channel::<u32>().map_err(|error| error.to_string())?;
    let task = thread.run_send(incoming, |mut incoming| async move {
        let replies = incoming.recv().await.unwrap().unwrap();
        replies.send(17).unwrap();
        replies.send(19).unwrap();
        drop(replies);
        assert!(incoming.recv().await.unwrap().is_none());
    });
    // Transferring an endpoint through a running channel uses the opposite Post
    // preparation order from Thread::run. Both must preserve endpoint ownership.
    requests.send(replies).map_err(|error| error.to_string())?;
    drop(requests);
    task.await.map_err(|error| error.to_string())?;
    assert_eq!(received.recv().await.unwrap(), Some(17));
    assert_eq!(received.recv().await.unwrap(), Some(19));
    assert!(received.recv().await.unwrap().is_none());
    assert!(received.recv().await.unwrap().is_none());

    let (sender, mut receiver) = channel::<u32>().map_err(|error| error.to_string())?;
    let mut waiting = Box::pin(receiver.recv());
    assert!(
        poll_fn(|context| std::task::Poll::Ready(waiting.as_mut().poll(context)))
            .await
            .is_pending()
    );
    drop(waiting);
    sender.send(23).unwrap();
    drop(sender);
    assert_eq!(receiver.recv().await.unwrap(), Some(23));
    assert!(receiver.recv().await.unwrap().is_none());

    #[cfg(target_arch = "wasm32")]
    assert!(thread.run_send(receiver, |_| async {}).await.is_err());
    Ok(())
}
