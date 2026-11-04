use futures_channel::oneshot;
use relocade_wasm_runtime::{
    worker::{wake_connection, Thread, ThreadExt, WakeTarget, WorkerResource},
    SharedMemory,
};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::Waker,
};

#[path = "worker_wakes/executor.rs"]
mod executor;

#[path = "worker_wakes/host_waits.rs"]
mod host_waits;

#[path = "worker_wakes/peers.rs"]
mod peers;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(Clone, WorkerResource)]
struct Signal {
    wakes: Arc<AtomicUsize>,
    lifetime: Arc<ResourceLifetime>,
    #[worker(post)]
    memory: SharedMemory,
}

impl Signal {
    fn new(released: oneshot::Sender<()>) -> Self {
        Self {
            wakes: Arc::default(),
            lifetime: Arc::new(ResourceLifetime {
                released: Some(released),
                wake: Mutex::new(None),
            }),
            memory: SharedMemory::new(1, 1).unwrap(),
        }
    }
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

struct ResourceLifetime {
    released: Option<oneshot::Sender<()>>,
    wake: Mutex<Option<Waker>>,
}
impl Drop for ResourceLifetime {
    fn drop(&mut self) {
        // A target destructor can itself wake through the connection being torn down.
        if let Some(wake) = self.wake.get_mut().unwrap().take() {
            wake.wake();
        }
        let _ = self.released.take().unwrap().send(());
    }
}

async fn fixed_binding() {
    let mut old: Option<Waker> = None;
    for drop_host_first in [false, true] {
        let (released, release) = oneshot::channel();
        let signal = Signal::new(released);
        let wakes = signal.wakes.clone();
        let memory = signal.memory.clone();
        let (host, endpoint) = wake_connection(signal.clone());
        let (send_waker, receive_waker) = oneshot::channel();
        let (continue_worker, continued) = oneshot::channel();
        let thread = Thread::new();
        let task = thread.run_resource(signal, move |signal| async move {
            let connection = endpoint.bind(signal.clone());
            connection.waker().wake_by_ref();
            assert_eq!(signal.memory.atomic_load::<u8>(0), 1);
            connection.acknowledge();
            *signal.lifetime.wake.lock().unwrap() = Some(connection.waker().clone());
            send_waker.send(connection.waker().clone()).unwrap();
            continued.await.unwrap();
            let before = signal.wakes.load(Ordering::SeqCst);
            connection.waker().wake_by_ref();
            assert_eq!(
                signal.wakes.load(Ordering::SeqCst),
                before + usize::from(!drop_host_first)
            );
        });
        // The endpoint binds without polling the join handle or a host receiver.
        let waker = receive_waker.await.unwrap();
        let before = wakes.load(Ordering::SeqCst);
        if let Some(old) = old.take() {
            old.wake();
            assert_eq!(wakes.load(Ordering::SeqCst), before);
        }
        waker.wake_by_ref();
        assert_eq!(wakes.load(Ordering::SeqCst), before + 1);
        assert_eq!(memory.atomic_load::<u8>(0), 1);
        let mut host = Some(host);
        if drop_host_first {
            drop(host.take());
        }
        continue_worker.send(()).unwrap();
        task.await.unwrap();
        let before = wakes.load(Ordering::SeqCst);
        waker.wake_by_ref();
        assert_eq!(
            wakes.load(Ordering::SeqCst),
            before,
            "completed connection is inactive"
        );
        drop(host);
        release.await.unwrap();
        old = Some(waker);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fixed_resources_notify_both_threads_and_end_with_the_connection() {
    pollster::block_on(fixed_binding());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn fixed_resources_notify_both_threads_and_end_with_the_connection() {
    fixed_binding().await;
}
