use futures_channel::oneshot;
use futures_util::FutureExt;
use relocade_workers::{ChannelError, Worker, WorkerError};
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
    let mut worker = Worker::spawn(|worker| {
        worker.run(move |mut stopped| async move {
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
    assert_eq!(worker.join().await.unwrap(), Some(42));
    assert!(released.load(Ordering::Acquire));
}

async fn cancellation_cleanup(setup_failure: bool) {
    let (cancelled, cancellation) = oneshot::channel();
    let (finish, finished) = oneshot::channel();
    let released = Arc::new(AtomicBool::new(false));
    let worker_released = released.clone();
    let worker = Worker::spawn(|worker| {
        worker.run(move |stopped| async move {
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
        assert_eq!(result.unwrap().as_deref(), Some("stopped"));
    }
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
    for stop_before_message in [true, false] {
        let (entered, mut started) = oneshot::channel();
        let mut input = None;
        let mut worker = Worker::spawn(|worker| {
            input =
                Some(worker.listen(move |value, stopped| wait_for_stop(value, stopped, entered))?);
            Ok(())
        });
        let mut input = input.unwrap();
        assert!(worker.finished().now_or_never().is_none());
        assert_eq!(started.try_recv().unwrap(), None);
        if stop_before_message {
            assert_eq!(worker.stop().await.unwrap(), None);
            assert!(
                started.await.is_err(),
                "a stopped worker must not enter its task"
            );
        } else {
            input.send(42).unwrap();
            started.await.unwrap();
            // Startup releases its one-use inbox before entering the task.
            assert!(matches!(input.send(99), Err(ChannelError::Closed)));
            assert_eq!(worker.stop().await.unwrap(), Some(42));
        }
        assert!(matches!(input.send(7), Err(ChannelError::Closed)));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dropping_a_completion_wait_preserves_the_task_and_result() {
    pollster::block_on(completion_wait());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn initial_messages_start_tasks_and_stop_can_prevent_startup() {
    pollster::block_on(initial_message());
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
async fn initial_messages_start_tasks_and_stop_can_prevent_startup() {
    initial_message().await;
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
