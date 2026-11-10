use futures_channel::oneshot;
use futures_util::{
    future::{select, Either},
    FutureExt,
};
use relocade_workers::{CancellableWorker, ChannelError, Receiver, WorkerError};
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
    let mut worker = CancellableWorker::spawn(|worker, mut stopped| {
        worker.run(move || async move {
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
        Ok(())
    });
    started.await.unwrap();
    assert!(worker.finished().now_or_never().is_none());
    assert!(!released.load(Ordering::Acquire));
    finish.send(()).unwrap();
    assert_eq!(worker.join().await.unwrap(), 42);
    assert!(released.load(Ordering::Acquire));
}

async fn cancellation_cleanup(setup_failure: bool) {
    let (cancelled, cancellation) = oneshot::channel();
    let (finish, finished) = oneshot::channel();
    let released = Arc::new(AtomicBool::new(false));
    let worker_released = released.clone();
    let mut input = None;
    let worker = CancellableWorker::spawn(|worker, stopped| {
        let sender = worker.listen(move |mut incoming: Receiver<String>| async move {
            let _locals = Rc::new(Locals {
                owner: thread::current().id(),
                released: worker_released,
            });
            // Stop must work before the first resource arrives, even while the
            // sender stays open. The owner still waits for asynchronous cleanup.
            let received = pin!(incoming.recv());
            assert!(matches!(select(stopped, received).await, Either::Left(_)));
            cancelled.send(()).unwrap();
            // Cancellation can require asynchronous cleanup on the worker.
            finished.await.unwrap();
            Ok(String::from("stopped"))
        })?;
        input = Some(sender);
        if setup_failure {
            Err(ChannelError::Closed)
        } else {
            Ok(())
        }
    });
    let mut stop = pin!(async move {
        if setup_failure {
            worker.join().await
        } else {
            worker.stop().await
        }
    });
    assert!(stop.as_mut().now_or_never().is_none());
    cancellation.await.unwrap();
    assert!(!released.load(Ordering::Acquire));
    finish.send(()).unwrap();
    let result = stop.await;
    if setup_failure {
        assert!(matches!(
            result,
            Err(WorkerError::Channel(ChannelError::Closed))
        ));
    } else {
        assert_eq!(result.unwrap(), "stopped");
    }
    assert!(released.load(Ordering::Acquire));
    assert!(input.unwrap().send(String::from("too late")).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dropping_a_completion_wait_preserves_the_task_and_result() {
    pollster::block_on(completion_wait());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn stopping_waits_for_worker_cleanup_and_returns_its_result() {
    pollster::block_on(cancellation_cleanup(false));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn startup_failure_cancels_the_task_and_waits_for_cleanup() {
    pollster::block_on(cancellation_cleanup(true));
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
    cancellation_cleanup(false).await;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn startup_failure_cancels_the_task_and_waits_for_cleanup() {
    cancellation_cleanup(true).await;
}
