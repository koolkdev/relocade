#![cfg(feature = "worker")]

use futures_util::FutureExt;
use relocade_compiler::abi::Memory as MemoryDeclaration;
use relocade_wasm_runtime::{
    worker::{channel, Thread, ThreadExt, WorkerResource},
    Backend, SharedMemory, WasmBackend,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(WorkerResource)]
struct Message {
    sequence: u32,
    dropped: CountDrop,
    #[worker(post)]
    memory: SharedMemory,
}

struct CountDrop(Arc<AtomicUsize>);
impl Drop for CountDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn memory(value: u8) -> SharedMemory {
    let mut backend = Backend::new().unwrap();
    let memory = backend
        .create_memory(MemoryDeclaration {
            maximum: Some(1),
            shared: true,
            ..MemoryDeclaration::new("test", "memory", 1)
        })
        .unwrap();
    let memory = backend.shared_memory(&memory).unwrap();
    memory.atomic_store(0, value);
    memory
}

async fn exchange() {
    let memories: Vec<_> = (1..=3).map(memory).collect();
    let drops = Arc::new(AtomicUsize::new(0));
    let (local, remote) = channel::<Message>().unwrap();
    let mut local = local.bind();
    // An abandoned receive must not consume a later message.
    assert!(local.recv().now_or_never().is_none());
    let thread = Thread::new();
    let task = thread.run_resource(remote, |remote| async move {
        let mut remote = remote.bind();
        for expected in 1..=3 {
            let message = remote.recv().await.unwrap().unwrap();
            assert_eq!(message.sequence, expected);
            assert_eq!(u32::from(message.memory.atomic_load::<u8>(0)), expected);
            remote.send(message).unwrap();
        }
        // Dropping the peer queues closure after the earlier resources.
    });
    for (sequence, memory) in (1..=3).zip(&memories) {
        local
            .send(Message {
                sequence,
                dropped: CountDrop(drops.clone()),
                memory: memory.clone(),
            })
            .unwrap();
    }
    for expected in 1..=3 {
        let message = local.recv().await.unwrap().unwrap();
        assert_eq!(message.sequence, expected);
        message.memory.atomic_store(4, expected as u8);
    }
    assert!(local.recv().await.is_none());
    task.await.unwrap();
    for (expected, memory) in (1..=3).zip(&memories) {
        assert_eq!(memory.atomic_load::<u8>(4), expected);
    }
    assert_eq!(drops.load(Ordering::SeqCst), 3);

    // A send on this thread queues Rust captures immediately. Browser delivery
    // of the posted half must wait until this poll returns to the event loop.
    let (sender, receiver) = channel::<Message>().unwrap();
    let mut sender = sender.bind();
    let mut receiver = receiver.bind();
    sender
        .send(Message {
            sequence: 4,
            dropped: CountDrop(drops.clone()),
            memory: memory(4),
        })
        .unwrap();
    drop(sender);
    #[cfg(target_arch = "wasm32")]
    assert!(receiver.recv().now_or_never().is_none());
    assert_eq!(receiver.recv().await.unwrap().unwrap().sequence, 4);
    assert!(receiver.recv().await.is_none());
    assert_eq!(drops.load(Ordering::SeqCst), 4);

    let (unbound, receiver) = channel::<Message>().unwrap();
    let mut receiver = receiver.bind();
    drop(unbound);
    assert!(receiver.recv().await.is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resource_channels_preserve_order_captures_and_close() {
    pollster::block_on(exchange());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn resource_channels_preserve_order_captures_and_close() {
    exchange().await;
}
