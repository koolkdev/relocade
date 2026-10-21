#![cfg(not(target_arch = "wasm32"))]

use relocade_worker::{Error, RequestWorker, Result};
use std::{sync::mpsc, time::Duration};

type Job = Box<dyn FnOnce() -> Result<i32> + Send>;

#[test]
fn submissions_do_not_wait_and_accepted_work_outlives_the_owner() {
    let mut worker = RequestWorker::spawn("test-requests", 2, |endpoint| {
        endpoint.serve(async |job: Job| job())
    })
    .unwrap();
    let (started, running) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let first = worker
        .submit(Box::new(move || {
            started.send(()).unwrap();
            wait.recv().unwrap();
            Ok(42)
        }))
        .unwrap()
        .unwrap();
    running.recv_timeout(Duration::from_secs(10)).unwrap();
    let second = worker.submit(Box::new(|| Ok(7))).unwrap().unwrap();
    assert!(worker.submit(Box::new(|| Ok(9))).unwrap().is_none());
    drop(worker);
    release.send(()).unwrap();
    assert_eq!(pollster::block_on(first).unwrap(), 42);
    assert_eq!(pollster::block_on(second).unwrap(), 7);
}

#[test]
fn failed_jobs_do_not_stop_the_worker() {
    let mut worker = RequestWorker::spawn("test-requests", 2, |endpoint| {
        endpoint.serve(async |job: Job| job())
    })
    .unwrap();
    let failed = worker
        .submit(Box::new(|| Err(Error::Failed("unsupported".into()))))
        .unwrap()
        .unwrap();
    assert!(pollster::block_on(failed).is_err());
    assert_eq!(
        pollster::block_on(worker.submit(Box::new(|| Ok(5))).unwrap().unwrap()).unwrap(),
        5
    );
}

#[test]
fn dropping_a_result_and_shutting_down_preserve_other_jobs() {
    let mut worker = RequestWorker::spawn("test-requests", 2, |endpoint| {
        endpoint.serve(async |job: Job| job())
    })
    .unwrap();
    pollster::block_on(worker.ready()).unwrap();
    let (started, running) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let ignored = worker
        .submit(Box::new(move || {
            started.send(()).unwrap();
            wait.recv().unwrap();
            Ok(11)
        }))
        .unwrap()
        .unwrap();
    running.recv_timeout(Duration::from_secs(10)).unwrap();
    drop(ignored);
    let accepted = worker.submit(Box::new(|| Ok(42))).unwrap().unwrap();
    worker.shutdown();
    let stopped = worker.stopped();
    release.send(()).unwrap();
    assert_eq!(pollster::block_on(accepted).unwrap(), 42);
    pollster::block_on(stopped).unwrap();
    assert!(worker.submit(Box::new(|| Ok(1))).is_err());
}

#[test]
fn a_worker_panic_fails_every_pending_result() {
    let mut worker = RequestWorker::spawn("test-requests", 2, |endpoint| {
        endpoint.serve(async |job: Job| job())
    })
    .unwrap();
    let (started, running) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let first = worker
        .submit(Box::new(move || {
            started.send(()).unwrap();
            wait.recv().unwrap();
            panic!("request panic");
        }))
        .unwrap()
        .unwrap();
    running.recv_timeout(Duration::from_secs(10)).unwrap();
    let second = worker.submit(Box::new(|| Ok(42))).unwrap().unwrap();
    release.send(()).unwrap();
    assert!(pollster::block_on(first)
        .unwrap_err()
        .to_string()
        .contains("request panic"));
    assert!(pollster::block_on(second)
        .unwrap_err()
        .to_string()
        .contains("request panic"));
}

#[test]
fn typed_entry_initializes_non_send_state_on_its_thread() {
    let caller_thread = std::thread::current().id();
    let mut worker = RequestWorker::spawn("local-state", 1, move |endpoint| async move {
        assert_ne!(std::thread::current().id(), caller_thread);
        let state = std::rc::Rc::new(std::cell::Cell::new(40));
        endpoint
            .serve(async |increment: i32| {
                state.set(state.get() + increment);
                Ok(state.get())
            })
            .await
    })
    .unwrap();
    assert_eq!(
        pollster::block_on(worker.submit(2).unwrap().unwrap()).unwrap(),
        42
    );
    worker.shutdown();
    pollster::block_on(worker.stopped()).unwrap();
}
