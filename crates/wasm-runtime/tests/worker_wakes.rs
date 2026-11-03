#![cfg(feature = "worker")]

use futures_channel::oneshot;
use futures_util::{
    future::{join, AbortHandle, Abortable},
    stream::FuturesUnordered,
    StreamExt,
};
use relocade_wasm_runtime::{
    worker::{wake_connection, Thread, ThreadExt, WakeTarget, WorkerResource},
    SharedMemory,
};
use std::{
    cell::Cell,
    future::{poll_fn, Future},
    pin::pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Wake, Waker},
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

struct Dropped(Arc<AtomicBool>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

async fn busy_session() {
    for cancel in [false, true] {
        let (released, _release) = oneshot::channel();
        let signal = Signal::new(released);
        let (_host, endpoint) = wake_connection(signal.clone());
        let (stop, cancelled) = AbortHandle::new_pair();
        let (send, input) = oneshot::channel();
        let (started, running) = oneshot::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let worker_drop = dropped.clone();
        let thread = Thread::new();
        let task = thread.run_resource(signal, move |signal| async move {
            let connection = endpoint.bind(signal.clone());
            let session = async {
                let _dropped = Dropped(worker_drop);
                // Abortable registers on Pending. Yield inside its child before
                // entering a synchronous interval that may need cancellation.
                connection.yield_now().await;
                let busy = async {
                    started.send(()).unwrap();
                    for _ in 0..50_000_000 {
                        if signal.memory.atomic_load::<u8>(0) != 0 {
                            return;
                        }
                        std::hint::spin_loop();
                    }
                    panic!("sender did not signal busy execution");
                };
                // Register the ordinary input receiver before entering busy work.
                join(input, busy).await.0.unwrap()
            };
            connection
                .run(Abortable::new(session, cancelled))
                .await
                .ok()
        });
        running.await.unwrap();
        if cancel {
            stop.abort();
        } else {
            send.send(42).unwrap();
        }
        assert_eq!(task.await.unwrap(), if cancel { None } else { Some(42) });
        assert!(dropped.load(Ordering::SeqCst));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn input_and_cancellation_wakes_interrupt_busy_sessions() {
    pollster::block_on(busy_session());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn input_and_cancellation_wakes_interrupt_busy_sessions() {
    busy_session().await;
}

async fn composition() {
    let (released, _release) = oneshot::channel();
    let signal = Signal::new(released);
    let (_host, endpoint) = wake_connection(signal.clone());
    Thread::new()
        .run_resource(signal, move |signal| async move {
            let connection = endpoint.bind(signal.clone());
            for (unordered, during_poll) in [(false, false), (true, false), (false, true)] {
                let (send, input) = oneshot::channel();
                let (send_between, send_during) = if during_poll {
                    (None, Some(send))
                } else {
                    (Some(send), None)
                };
                let consumed = Cell::new(false);
                let cpu = async {
                    connection.yield_now().await;
                    assert_ne!(
                        signal.memory.atomic_load::<u8>(0),
                        0,
                        "ready input must interrupt CPU-first composition"
                    );
                    connection.yield_now().await;
                    assert!(consumed.get(), "ready sibling must run before CPU resumes");
                    if !unordered {
                        assert_eq!(
                            signal.memory.atomic_load::<u8>(0),
                            0,
                            "our own cooperative yield must not request another interrupt"
                        );
                    }
                };
                let cpu = async {
                    if unordered {
                        let mut tasks = FuturesUnordered::new();
                        tasks.push(cpu);
                        tasks.next().await.unwrap();
                    } else {
                        cpu.await;
                    }
                };
                let receive = async {
                    assert_eq!(input.await.unwrap(), 42);
                    consumed.set(true);
                };
                let late_send = async {
                    // The receiver was already polled Pending this turn.
                    if let Some(send) = send_during {
                        send.send(42).unwrap();
                    }
                };
                let mut session = pin!(connection.run(join(cpu, join(receive, late_send))));
                // Exact polls detect a lost child wake without a timeout or budget.
                assert!(poll_fn(|cx| Poll::Ready(session.as_mut().poll(cx)))
                    .await
                    .is_pending());
                if let Some(send) = send_between {
                    send.send(42).unwrap();
                }
                assert!(poll_fn(|cx| Poll::Ready(session.as_mut().poll(cx)))
                    .await
                    .is_pending());
                assert!(poll_fn(|cx| Poll::Ready(session.as_mut().poll(cx)))
                    .await
                    .is_ready());
            }
        })
        .await
        .unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn late_input_and_nested_combinators_preserve_wakes() {
    pollster::block_on(composition());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn late_input_and_nested_combinators_preserve_wakes() {
    composition().await;
}

struct DeliverInput(Mutex<Option<oneshot::Sender<()>>>);
impl Wake for DeliverInput {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        let input = self.0.lock().unwrap().take();
        if let Some(input) = input {
            input.send(()).unwrap();
        }
    }
}

async fn reentrant_input() {
    let (released, _release) = oneshot::channel();
    let signal = Signal::new(released);
    let (_host, endpoint) = wake_connection(signal.clone());
    Thread::new()
        .run_resource(signal, move |signal| async move {
            let connection = endpoint.bind(signal.clone());
            let (send, receive) = oneshot::channel();
            let waker = Waker::from(Arc::new(DeliverInput(Mutex::new(Some(send)))));
            let mut context = Context::from_waker(&waker);
            // The input waiter is registered before yield wakes the executor. The
            // executor publishes input synchronously during that same wake chain.
            let mut session = pin!(connection.run(join(receive, connection.yield_now())));
            assert!(session.as_mut().poll(&mut context).is_pending());
            assert_eq!(
                signal.memory.atomic_load::<u8>(0),
                1,
                "suppressing our yield must not suppress the executor's separate input wake"
            );
            assert_eq!(
                session.as_mut().poll(&mut context),
                Poll::Ready((Ok(()), ()))
            );
        })
        .await
        .unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_executor_wake_can_publish_input_during_cooperative_yield() {
    pollster::block_on(reentrant_input());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn an_executor_wake_can_publish_input_during_cooperative_yield() {
    reentrant_input().await;
}
