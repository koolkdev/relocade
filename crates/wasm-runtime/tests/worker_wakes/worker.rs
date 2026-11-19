use super::*;
use futures_util::{
    future::{join, select, Either},
    stream::FuturesUnordered,
    StreamExt,
};
use relocade_wasm_runtime::worker::WorkerWake;
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    pin::pin,
    sync::atomic::AtomicBool,
    task::{Context, Poll, Wake},
};

async fn drive<F: Future>(worker: &WorkerWake<Signal>, future: F) -> F::Output {
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(result) = worker.poll(|cx| future.as_mut().poll(cx)) {
            return result;
        }
        worker.wait().await;
    }
}

#[derive(Default)]
struct CountWakes(AtomicUsize);
impl Wake for CountWakes {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
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
        let (stop, cancelled) = oneshot::channel::<()>();
        let (send, input) = oneshot::channel();
        let (started, running) = oneshot::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let worker_drop = dropped.clone();
        let worker = Thread::new();
        let (mut incoming, task) = worker
            .listen(move |mut incoming: Receiver<Signal>| async move {
                let signal = incoming.recv().await.unwrap().unwrap();
                let worker = WorkerWake::new(endpoint.bind(signal.clone()));
                let session = async move {
                    let _dropped = Dropped(worker_drop);
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
                match drive(&worker, select(cancelled, pin!(session))).await {
                    Either::Left(_) => None,
                    Either::Right((value, _)) => Some(value),
                }
            })
            .unwrap();
        incoming.send(signal).unwrap();
        running.await.unwrap();
        if cancel {
            stop.send(()).unwrap();
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
    let worker = Thread::new();
    let (mut incoming, task) = worker
        .listen(move |mut incoming: Receiver<Signal>| async move {
            let signal = incoming.recv().await.unwrap().unwrap();
            let worker = WorkerWake::new(endpoint.bind(signal.clone()));
            let (send, input) = oneshot::channel();
            let mut tasks = FuturesUnordered::new();
            tasks.push(input);
            worker.poll(|cx| {
                assert!(tasks.poll_next_unpin(cx).is_pending());
                // This nested task has already been polled this turn.
                send.send(42).unwrap();
                assert_eq!(signal.memory.atomic_load::<u8>(0), 1);
            });
            assert_eq!(signal.memory.atomic_load::<u8>(0), 1);
            assert!(worker.has_pending_poll());
            worker.wait().await;
            worker.poll(|cx| {
                assert_eq!(tasks.poll_next_unpin(cx), Poll::Ready(Some(Ok(42))));
            });
            assert_eq!(signal.memory.atomic_load::<u8>(0), 0);
            worker.request_poll();
            assert_eq!(signal.memory.atomic_load::<u8>(0), 0);
            worker.wait().await;
        })
        .unwrap();
    incoming.send(signal).unwrap();
    task.await.unwrap();
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
    let worker = Thread::new();
    let (mut incoming, task) = worker
        .listen(move |mut incoming: Receiver<Signal>| async move {
            let signal = incoming.recv().await.unwrap().unwrap();
            let worker = WorkerWake::new(endpoint.bind(signal.clone()));
            let (trigger, triggered) = oneshot::channel();
            let (send, receive) = oneshot::channel();
            let waker = Waker::from(Arc::new(DeliverInput(Mutex::new(Some(send)))));
            let mut context = Context::from_waker(&waker);
            // The parked worker publishes another input synchronously while
            // the first wake is still unwinding through the worker waker.
            let mut session = pin!(drive(&worker, join(triggered, receive)));
            assert!(session.as_mut().poll(&mut context).is_pending());
            trigger.send(()).unwrap();
            assert_eq!(
                signal.memory.atomic_load::<u8>(0),
                1,
                "both inputs must remain visible when leaving the parked state"
            );
            assert_eq!(
                session.as_mut().poll(&mut context),
                Poll::Ready((Ok(()), Ok(())))
            );
        })
        .unwrap();
    incoming.send(signal).unwrap();
    task.await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_parked_worker_wake_can_publish_another_input() {
    pollster::block_on(reentrant_input());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_parked_worker_wake_can_publish_another_input() {
    reentrant_input().await;
}

async fn parking_lifetime() {
    let (released, _release) = oneshot::channel();
    let signal = Signal::new(released);
    let (_host, endpoint) = wake_connection(signal.clone());
    let worker = Thread::new();
    let (mut incoming, task) = worker
        .listen(move |mut incoming: Receiver<Signal>| async move {
            let signal = incoming.recv().await.unwrap().unwrap();
            let worker = WorkerWake::new(endpoint.bind(signal));
            for complete in [false, true] {
                let (send, input) = oneshot::channel();
                let mut input = pin!(input);
                let captured = RefCell::new(None::<Waker>);
                let dropped = Arc::new(AtomicBool::new(false));
                let guard = Dropped(dropped.clone());
                let root = async {
                    let _guard = guard;
                    poll_fn(|cx| {
                        let mut captured = captured.borrow_mut();
                        if let Some(waker) = &*captured {
                            assert!(waker.will_wake(cx.waker()), "worker waker must stay stable");
                        } else {
                            *captured = Some(cx.waker().clone());
                        }
                        input.as_mut().poll(cx)
                    })
                    .await
                };
                let mut session = Box::pin(drive(&worker, root));
                let first = Arc::new(CountWakes::default());
                let first_weak = Arc::downgrade(&first);
                let first_waker = Waker::from(first.clone());
                assert!(session
                    .as_mut()
                    .poll(&mut Context::from_waker(&first_waker))
                    .is_pending());
                let second = Arc::new(CountWakes::default());
                let second_weak = Arc::downgrade(&second);
                let second_waker = Waker::from(second.clone());
                assert!(session
                    .as_mut()
                    .poll(&mut Context::from_waker(&second_waker))
                    .is_pending());
                drop(first_waker);
                drop(first);
                assert!(
                    first_weak.upgrade().is_none(),
                    "replaced waiter must be released"
                );
                let escaped = captured.borrow().as_ref().unwrap().clone();
                if complete {
                    send.send(42).unwrap();
                    assert_eq!(second.0.load(Ordering::SeqCst), 1);
                    assert_eq!(
                        session
                            .as_mut()
                            .poll(&mut Context::from_waker(&second_waker)),
                        Poll::Ready(Ok(42))
                    );
                }
                drop(session);
                assert!(dropped.load(Ordering::SeqCst));
                let before = second.0.load(Ordering::SeqCst);
                escaped.wake_by_ref();
                assert_eq!(second.0.load(Ordering::SeqCst), before);
                drop(second_waker);
                drop(second);
                assert!(
                    second_weak.upgrade().is_none(),
                    "escaped worker wakers must not retain the outer task"
                );
            }
        })
        .unwrap();
    incoming.send(signal).unwrap();
    task.await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn parked_waiters_can_change_and_are_released_on_completion_or_cancellation() {
    pollster::block_on(parking_lifetime());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn parked_waiters_can_change_and_are_released_on_completion_or_cancellation() {
    parking_lifetime().await;
}

async fn idle_session() {
    let (released, _release) = oneshot::channel();
    let signal = Signal::new(released);
    let (_host, endpoint) = wake_connection(signal.clone());
    let (send, input) = oneshot::channel();
    let (parked, idle) = oneshot::channel();
    let worker = Thread::new();
    let (mut incoming, task) = worker
        .listen(move |mut incoming: Receiver<Signal>| async move {
            let signal = incoming.recv().await.unwrap().unwrap();
            let worker = WorkerWake::new(endpoint.bind(signal));
            let mut session = pin!(drive(&worker, input));
            assert!(poll_fn(|cx| Poll::Ready(session.as_mut().poll(cx)))
                .await
                .is_pending());
            parked.send(()).unwrap();
            session.await.unwrap()
        })
        .unwrap();
    incoming.send(signal).unwrap();
    idle.await.unwrap();
    send.send(42).unwrap();
    assert_eq!(task.await.unwrap(), 42);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn host_input_wakes_an_idle_worker() {
    pollster::block_on(idle_session());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn host_input_wakes_an_idle_worker() {
    idle_session().await;
}
