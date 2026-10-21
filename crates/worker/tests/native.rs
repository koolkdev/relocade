#![cfg(not(target_arch = "wasm32"))]

use relocade_worker::{native::Worker, Error};
use std::{
    cell::Cell,
    future::Future,
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

#[test]
fn messages_and_completion_outlive_the_submission_owner() {
    pollster::block_on(async {
        let caller = std::thread::current().id();
        let (worker, mut events) = Worker::spawn("test-worker", move |mut endpoint| async move {
            assert_ne!(std::thread::current().id(), caller);
            let state = Rc::new(Cell::new(40));
            endpoint.ready();
            while let Some(value) = endpoint.recv().await {
                state.set(state.get() + value);
                endpoint.send(state.get())?;
            }
            Ok(())
        })
        .unwrap();
        // Neither dropped lifecycle waiters nor an unpolled receiver stop work.
        drop(worker.ready());
        worker.ready().await.unwrap();
        worker.send(2).unwrap();
        worker.send(3).unwrap();
        let stopped = worker.stopped();
        drop(worker);
        assert_eq!(events.recv().await, Some(42));
        assert_eq!(events.recv().await, Some(45));
        assert_eq!(events.recv().await, None);
        stopped.await.unwrap();
    });
}

#[test]
fn shutdown_before_readiness_drains_input_and_rejects_new_messages() {
    pollster::block_on(async {
        let (initialize, gate) = futures_channel::oneshot::channel();
        let (mut worker, mut events) =
            Worker::spawn("test-worker", move |mut endpoint| async move {
                gate.await.unwrap();
                endpoint.ready();
                while let Some(value) = endpoint.recv().await {
                    endpoint.send(value)?;
                }
                Ok(())
            })
            .unwrap();
        assert!(pin!(worker.ready())
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        worker.send(42).unwrap();
        worker.shutdown();
        assert_eq!(worker.send(7), Err(Error::Closed));
        initialize.send(()).unwrap();
        worker.ready().await.unwrap();
        worker.stopped().await.unwrap();
        assert_eq!(events.recv().await, Some(42));
        assert_eq!(events.recv().await, None);
    });
}

#[test]
fn cancelled_receive_does_not_consume_the_next_event() {
    pollster::block_on(async {
        let (mut worker, mut events) = Worker::spawn("test-worker", |mut endpoint| async move {
            endpoint.ready();
            while let Some(value) = endpoint.recv().await {
                endpoint.send(value)?;
            }
            Ok(())
        })
        .unwrap();
        worker.ready().await.unwrap();
        assert!(matches!(
            pin!(events.recv())
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ));
        worker.send(42).unwrap();
        assert_eq!(events.recv().await, Some(42));
        worker.shutdown();
        worker.stopped().await.unwrap();
    });
}

#[test]
fn startup_failure_and_panics_complete_lifecycle_waiters() {
    pollster::block_on(async {
        let (worker, _) = Worker::<()>::spawn::<(), _, _>("failed-worker", |_| async {
            Err(Error::Failed("initialization failed".into()))
        })
        .unwrap();
        let stopped = worker.stopped();
        assert_eq!(
            worker.ready().await,
            Err(Error::Failed("initialization failed".into()))
        );
        assert_eq!(
            stopped.await,
            Err(Error::Failed("initialization failed".into()))
        );
        assert_eq!(worker.send(()), Err(Error::Closed));

        let (worker, mut events) =
            Worker::<()>::spawn::<(), _, _>("panicking-worker", |endpoint| async move {
                endpoint.ready();
                panic!("entry panic");
            })
            .unwrap();
        worker.ready().await.unwrap();
        assert_eq!(
            worker.stopped().await,
            Err(Error::Failed("entry panic".into()))
        );
        assert_eq!(events.recv().await, None);
    });
}
