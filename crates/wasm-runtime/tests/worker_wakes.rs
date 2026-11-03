#![cfg(feature = "worker")]

use futures_channel::oneshot;
use futures_util::{
    future::{join, AbortHandle, Abortable},
    FutureExt,
};
use relocade_compiler::abi::Memory as MemoryDeclaration;
use relocade_wasm_runtime::{
    worker::{wake_connection, Thread, ThreadExt, WakeTarget, WorkerResource},
    Backend, SharedMemory, WasmBackend,
};
use std::{
    future::poll_fn,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::Waker,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(Clone, WorkerResource)]
struct Signal {
    wakes: Arc<AtomicUsize>,
    lifetime: Arc<ResourceLifetime>,
    #[worker(post)]
    memory: SharedMemory,
}

impl WakeTarget for Signal {
    fn wake(&self) {
        self.wakes.fetch_add(1, Ordering::SeqCst);
        self.memory.atomic_store(0, 1_u8);
    }

    fn acknowledge(&self) {
        self.memory.atomic_swap(0, 0_u8);
    }
}

impl Signal {
    fn new(wakes: Arc<AtomicUsize>, released: oneshot::Sender<()>) -> Self {
        let mut backend = Backend::new().unwrap();
        let memory = backend
            .create_memory(MemoryDeclaration {
                maximum: Some(1),
                shared: true,
                ..MemoryDeclaration::new("test", "signal", 1)
            })
            .unwrap();
        Self {
            wakes,
            lifetime: Arc::new(ResourceLifetime {
                released: Some(released),
                wake: Mutex::new(None),
            }),
            memory: backend.shared_memory(&memory).unwrap(),
        }
    }
}

struct ResourceLifetime {
    released: Option<oneshot::Sender<()>>,
    wake: Mutex<Option<Waker>>,
}
impl Drop for ResourceLifetime {
    fn drop(&mut self) {
        // Removing the last target must release connection guards before its
        // destructor wakes a task through the same connection.
        if let Some(wake) = self.wake.get_mut().unwrap().take() {
            wake.wake();
        }
        let _ = self.released.take().unwrap().send(());
    }
}

struct Dropped(Arc<AtomicBool>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

async fn exercise() {
    let mut old: Option<Waker> = None;
    for cancel in [false, true] {
        let (_host, endpoint) = wake_connection::<Signal>().unwrap();
        let (released, release) = oneshot::channel();
        let (stop, cancelled) = AbortHandle::new_pair();
        let (send, mut input) = oneshot::channel();
        let (started, running) = oneshot::channel();
        let (capture, captured) = oneshot::channel();
        let wakes = Arc::new(AtomicUsize::new(0));
        let worker_wakes = wakes.clone();
        let dropped = Arc::new(AtomicBool::new(false));
        let worker_drop = dropped.clone();
        let thread = Thread::new();
        let task = thread.run_resource(endpoint, move |endpoint| async move {
            let connection = endpoint.bind();
            let target = Signal::new(worker_wakes, released);
            let mut capture = Some(capture);
            let session = async {
                let _dropped = Dropped(worker_drop);
                connection.activate(target.clone()).await.unwrap();
                // Register the ordinary oneshot under the wrapped root waker
                // before spending a synchronous interval outside the executor.
                let receive = poll_fn(|cx| {
                    if let Some(capture) = capture.take() {
                        *target.lifetime.wake.lock().unwrap() = Some(cx.waker().clone());
                        capture.send(cx.waker().clone()).unwrap();
                    }
                    input.poll_unpin(cx)
                });
                let busy = async {
                    started.send(()).unwrap();
                    for _ in 0..50_000_000 {
                        if target.memory.atomic_load::<u8>(0) != 0 {
                            return;
                        }
                        std::hint::spin_loop();
                    }
                    panic!("sender did not signal busy execution");
                };
                join(receive, busy).await.0.unwrap()
            };
            let result = connection.run(Abortable::new(session, cancelled)).await;
            if result.is_ok() {
                assert_eq!(
                    target.memory.atomic_load::<u8>(0),
                    0,
                    "acknowledge before polling ready work"
                );
            }
            result.ok()
        });
        // Startup and resource installation proceed before task/join is polled.
        running.await.unwrap();
        let before = wakes.load(Ordering::SeqCst);
        if let Some(old) = old.take() {
            old.wake();
            assert_eq!(
                wakes.load(Ordering::SeqCst),
                before,
                "old session cannot signal its replacement"
            );
        }
        old = Some(captured.await.unwrap());
        if cancel {
            stop.abort();
        } else {
            send.send(42).unwrap();
        }
        assert_eq!(task.await.unwrap(), if cancel { None } else { Some(42) });
        assert!(dropped.load(Ordering::SeqCst));
        // Completion releases both local targets even while the host guard and
        // an escaped Waker remain alive. The waker owns no resource handle.
        release.await.unwrap();
        drop(thread);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ordinary_wakes_and_cancellation_signal_a_connected_busy_worker() {
    pollster::block_on(exercise());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn ordinary_wakes_and_cancellation_signal_a_connected_busy_worker() {
    exercise().await;
}
