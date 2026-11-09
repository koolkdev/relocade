use futures_channel::oneshot;
use futures_util::FutureExt;
use relocade_workers::CancellableWorker;
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
    let mut worker = CancellableWorker::spawn(|thread, mut stopped| {
        thread.run_send((), move |()| async move {
            // Worker-local futures can retain non-Send resources across awaits.
            let _locals = Rc::new(Locals {
                owner: thread::current().id(),
                released: worker_released,
            });
            ready.send(()).unwrap();
            finished.await.unwrap();
            assert_eq!(stopped.try_recv().unwrap(), None);
            42
        })
    });
    started.await.unwrap();
    assert!(worker.finished().now_or_never().is_none());
    assert!(!released.load(Ordering::Acquire));
    finish.send(()).unwrap();
    assert_eq!(worker.join().await.unwrap(), 42);
    assert!(released.load(Ordering::Acquire));
}

async fn cancellation_cleanup() {
    let (cancelled, cancellation) = oneshot::channel();
    let (finish, finished) = oneshot::channel();
    let released = Arc::new(AtomicBool::new(false));
    let worker_released = released.clone();
    let worker = CancellableWorker::spawn(|thread, stopped| {
        thread.run_send((), move |()| async move {
            let _locals = Rc::new(Locals {
                owner: thread::current().id(),
                released: worker_released,
            });
            let _ = stopped.await;
            cancelled.send(()).unwrap();
            // Cancellation can require asynchronous cleanup on the worker.
            finished.await.unwrap();
            String::from("stopped")
        })
    });
    let mut stop = pin!(worker.stop());
    assert!(stop.as_mut().now_or_never().is_none());
    cancellation.await.unwrap();
    assert!(!released.load(Ordering::Acquire));
    finish.send(()).unwrap();
    assert_eq!(stop.await.unwrap(), "stopped");
    assert!(released.load(Ordering::Acquire));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dropping_a_completion_wait_preserves_the_task_and_result() {
    pollster::block_on(completion_wait());
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
async fn stopping_waits_for_worker_cleanup_and_returns_its_result() {
    cancellation_cleanup().await;
}
