use futures_util::FutureExt;
use relocade_workers::{JobQueue, PoolError, Worker};
use std::sync::{Arc, Mutex};

async fn reusable_thread() {
    let (queue, jobs) = JobQueue::new();
    let worker = Worker::spawn(move |stopped| queue.run(stopped));
    let (owner, state) = jobs
        .execute(|| (std::thread::current().id(), vec![1]))
        .await
        .unwrap();
    assert_ne!(owner, std::thread::current().id());
    let state = jobs
        .execute(move || {
            assert_eq!(std::thread::current().id(), owner);
            let mut state = state;
            state.push(2);
            state
        })
        .await
        .unwrap();
    assert_eq!(state, [1, 2]);
    drop(jobs);
    worker.join().await.unwrap();
}

async fn stop_drains_accepted_jobs() {
    let (queue, jobs) = JobQueue::new();
    let order = Arc::new(Mutex::new(Vec::new()));
    let first_order = order.clone();
    let mut first = Box::pin(jobs.execute(move || {
        first_order.lock().unwrap().push(1);
        42
    }));
    let second_order = order.clone();
    let mut second = Box::pin(jobs.execute(move || second_order.lock().unwrap().push(2)));
    assert!(first.as_mut().now_or_never().is_none());
    assert!(second.as_mut().now_or_never().is_none());
    drop(second);
    // Accepted work must finish even when its waiter disappears or stop wins startup.
    let worker = Worker::spawn(move |stopped| queue.run(stopped));
    worker.stop().await.unwrap();
    assert_eq!(first.await.unwrap(), 42);
    assert_eq!(*order.lock().unwrap(), [1, 2]);
    assert!(matches!(jobs.execute(|| ()).await, Err(PoolError::Closed)));
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn sequential_jobs_return_owned_state_on_one_reused_thread() {
    pollster::block_on(reusable_thread());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn sequential_jobs_return_owned_state_on_one_reused_thread() {
    reusable_thread().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn stopping_drains_accepted_work_and_closes_escaped_handles() {
    pollster::block_on(stop_drains_accepted_jobs());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn stopping_drains_accepted_work_and_closes_escaped_handles() {
    stop_drains_accepted_jobs().await;
}
