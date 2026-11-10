use super::*;
use futures_util::{future::join, stream::FuturesUnordered, StreamExt};
use relocade_wasm_runtime::worker::Executor;
use std::{
    cell::{Cell, RefCell},
    future::{pending, poll_fn, Future},
    pin::pin,
    sync::atomic::AtomicBool,
    task::{Context, Poll, Wake},
};

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
        let thread = Worker::new();
        let (mut resources, mut resource_input) = thread.channel().await.unwrap();
        resources.send(signal).unwrap();
        let task = thread.run(move || async move {
            let signal = resource_input.recv().await.unwrap().unwrap();
            let executor = Executor::new(endpoint.bind(signal.clone()));
            let session = async {
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
            executor.run_until(cancelled, session).await
        });
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
    let thread = Worker::new();
    let (mut resources, mut resource_input) = thread.channel().await.unwrap();
    resources.send(signal).unwrap();
    thread
        .run(move || async move {
            let signal = resource_input.recv().await.unwrap().unwrap();
            let executor = Executor::new(endpoint.bind(signal.clone()));
            for (unordered, during_poll) in [(false, false), (true, false), (false, true)] {
                let (send, input) = oneshot::channel();
                let (start, started) = oneshot::channel();
                let (send_between, send_during) = if during_poll {
                    (None, Some(send))
                } else {
                    (Some(send), None)
                };
                let consumed = Cell::new(false);
                let cpu = async {
                    if during_poll {
                        executor.yield_now().await;
                    } else {
                        started.await.unwrap();
                    }
                    assert_ne!(
                        signal.memory.atomic_load::<u8>(0),
                        0,
                        "ready input must interrupt CPU-first composition"
                    );
                    executor.yield_now().await;
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
                let mut session =
                    pin!(executor.run_until(pending::<()>(), join(cpu, join(receive, late_send))));
                let outer = Arc::new(CountWakes::default());
                let waker = Waker::from(outer.clone());
                let mut context = Context::from_waker(&waker);
                if let Some(send) = send_between {
                    assert!(session.as_mut().poll(&mut context).is_pending());
                    send.send(42).unwrap();
                    start.send(()).unwrap();
                    outer.0.store(0, Ordering::SeqCst);
                }
                assert!(session.as_mut().poll(&mut context).is_ready());
                assert_eq!(
                    outer.0.load(Ordering::SeqCst),
                    0,
                    "busy repolling must stay inside the worker executor"
                );
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
    let thread = Worker::new();
    let (mut resources, mut resource_input) = thread.channel().await.unwrap();
    resources.send(signal).unwrap();
    thread
        .run(move || async move {
            let signal = resource_input.recv().await.unwrap().unwrap();
            let executor = Executor::new(endpoint.bind(signal.clone()));
            let (trigger, triggered) = oneshot::channel();
            let (send, receive) = oneshot::channel();
            let waker = Waker::from(Arc::new(DeliverInput(Mutex::new(Some(send)))));
            let mut context = Context::from_waker(&waker);
            // The parked executor publishes another input synchronously while
            // the first wake is still unwinding through the executor waker.
            let mut session = pin!(executor.run_until(pending::<()>(), async {
                executor.yield_now().await;
                join(triggered, receive).await
            }));
            assert!(session.as_mut().poll(&mut context).is_pending());
            trigger.send(()).unwrap();
            assert_eq!(
                signal.memory.atomic_load::<u8>(0),
                1,
                "both inputs must remain visible when leaving the parked state"
            );
            assert_eq!(
                session.as_mut().poll(&mut context),
                Poll::Ready(Some((Ok(()), Ok(()))))
            );
        })
        .await
        .unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_parked_executor_wake_can_publish_another_input() {
    pollster::block_on(reentrant_input());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_parked_executor_wake_can_publish_another_input() {
    reentrant_input().await;
}

async fn parking_lifetime() {
    let (released, _release) = oneshot::channel();
    let signal = Signal::new(released);
    let (_host, endpoint) = wake_connection(signal.clone());
    let thread = Worker::new();
    let (mut resources, mut resource_input) = thread.channel().await.unwrap();
    resources.send(signal).unwrap();
    thread
        .run(move || async move {
            let signal = resource_input.recv().await.unwrap().unwrap();
            let executor = Executor::new(endpoint.bind(signal));
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
                            assert!(
                                waker.will_wake(cx.waker()),
                                "executor waker must stay stable"
                            );
                        } else {
                            *captured = Some(cx.waker().clone());
                        }
                        input.as_mut().poll(cx)
                    })
                    .await
                };
                let mut session = Box::pin(executor.run_until(pending::<()>(), root));
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
                        Poll::Ready(Some(Ok(42)))
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
                    "escaped executor wakers must not retain the outer task"
                );
            }
        })
        .await
        .unwrap();
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
    let thread = Worker::new();
    let (mut resources, mut resource_input) = thread.channel().await.unwrap();
    resources.send(signal).unwrap();
    let task = thread.run(move || async move {
        let signal = resource_input.recv().await.unwrap().unwrap();
        let executor = Executor::new(endpoint.bind(signal));
        assert_eq!(
            executor
                .run_until(std::future::ready(()), async {
                    panic!("a stopped session must not enter application code");
                })
                .await,
            None::<()>
        );
        let mut session = pin!(executor.run_until(pending::<()>(), input));
        assert!(poll_fn(|cx| Poll::Ready(session.as_mut().poll(cx)))
            .await
            .is_pending());
        parked.send(()).unwrap();
        session.await.unwrap().unwrap()
    });
    idle.await.unwrap();
    send.send(42).unwrap();
    assert_eq!(task.await.unwrap(), 42);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn host_input_resumes_an_idle_session_through_the_worker_executor() {
    pollster::block_on(idle_session());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn host_input_resumes_an_idle_session_through_the_worker_executor() {
    idle_session().await;
}
