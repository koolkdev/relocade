use futures_channel::oneshot;
use futures_util::FutureExt;
use relocade_workers::Worker;
use std::{
    pin::pin,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, ThreadId},
};

struct Locals {
    owner: ThreadId,
    released: Arc<AtomicBool>,
}

impl Drop for Locals {
    fn drop(&mut self) {
        assert_eq!(thread::current().id(), self.owner);
        self.released.store(true, Ordering::Release);
    }
}

async fn completion_wait() {
    let (ready, started) = oneshot::channel();
    let (finish, finished) = oneshot::channel();
    let released = Arc::new(AtomicBool::new(false));
    let worker_released = released.clone();
    let mut worker = Worker::spawn(move |mut stopped| async move {
        // Worker-local futures can retain non-Send resources across awaits.
        let _locals = Rc::new(Locals {
            owner: thread::current().id(),
            released: worker_released,
        });
        ready.send(()).unwrap();
        finished.await.unwrap();
        assert_eq!(stopped.try_recv().unwrap(), None);
        42
    });
    started.await.unwrap();
    assert!(worker.finished().now_or_never().is_none());
    assert!(!released.load(Ordering::Acquire));
    finish.send(()).unwrap();
    assert_eq!(worker.join().await.unwrap(), Some(42));
    assert!(released.load(Ordering::Acquire));
}

async fn cancellation_cleanup() {
    let (cancelled, cancellation) = oneshot::channel();
    let (finish, finished) = oneshot::channel();
    let released = Arc::new(AtomicBool::new(false));
    let worker_released = released.clone();
    let worker = Worker::spawn(move |stopped| async move {
        let _locals = Rc::new(Locals {
            owner: thread::current().id(),
            released: worker_released,
        });
        let _ = stopped.await;
        cancelled.send(()).unwrap();
        // Cancellation can require asynchronous cleanup on the worker.
        finished.await.unwrap();
        String::from("stopped")
    });
    let mut stop = pin!(worker.stop());
    assert!(stop.as_mut().now_or_never().is_none());
    cancellation.await.unwrap();
    assert!(!released.load(Ordering::Acquire));
    finish.send(()).unwrap();
    assert_eq!(stop.await.unwrap().as_deref(), Some("stopped"));
    assert!(released.load(Ordering::Acquire));
}

async fn wait_for_stop(
    value: u32,
    stopped: oneshot::Receiver<()>,
    entered: oneshot::Sender<()>,
) -> u32 {
    entered.send(()).unwrap();
    let _ = stopped.await;
    value
}

async fn initial_message() {
    for stop_immediately in [false, true] {
        let (entered, started) = oneshot::channel();
        let worker = Worker::spawn_with(42, move |value, stopped| {
            wait_for_stop(value, stopped, entered)
        });
        if stop_immediately {
            // Initial delivery may win the race with stop. The result must
            // accurately report whether the task entered and received its value.
            match worker.stop().await.unwrap() {
                None => assert!(started.await.is_err()),
                Some(value) => {
                    assert_eq!(value, 42);
                    started.await.unwrap();
                }
            }
        } else {
            started.await.unwrap();
            assert_eq!(worker.stop().await.unwrap(), Some(42));
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dropping_a_completion_wait_preserves_the_task_and_result() {
    pollster::block_on(completion_wait());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn workers_receive_initial_values_and_stop_cleanly() {
    pollster::block_on(initial_message());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn stopping_waits_for_worker_cleanup_and_returns_its_result() {
    pollster::block_on(cancellation_cleanup());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn dropping_a_completion_wait_preserves_the_task_and_result() {
    completion_wait().await;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn workers_receive_initial_values_and_stop_cleanly() {
    initial_message().await;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn stopping_waits_for_worker_cleanup_and_returns_its_result() {
    cancellation_cleanup().await;
}

#[cfg(target_arch = "wasm32")]
#[path = "worker/web.rs"]
mod web;
