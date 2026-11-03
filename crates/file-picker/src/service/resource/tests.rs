use crate::FileService;
use futures_channel::oneshot;
use futures_lite::future::{block_on, poll_once, zip};
use std::{cell::Cell, pin::pin, rc::Rc};

#[test]
fn worker_calls_borrow_local_state_in_order_on_its_owner() {
    block_on(async {
        let files = FileService::new();
        let state = Rc::new(Cell::new(0));
        let resource = files.resources.register(state.clone()).unwrap();
        let owner = std::thread::current().id();
        let (entered, started) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let (queued, waiting) = oneshot::channel();
        let thread = web_thread::Thread::new();
        let task = thread.run_send((), move |()| async move {
            let mut first = pin!(resource.call(async move |state| {
                assert_eq!(std::thread::current().id(), owner);
                state.set(1);
                entered.send(()).unwrap();
                released.await.unwrap();
                assert_eq!(state.get(), 1);
                state.set(2);
                Ok(())
            }));
            assert!(poll_once(&mut first).await.is_none());
            started.await.unwrap();
            let mut second = pin!(resource.call(async move |state| {
                assert_eq!(std::thread::current().id(), owner);
                assert_eq!(state.get(), 2);
                state.set(3);
                Ok(())
            }));
            assert!(poll_once(&mut second).await.is_none());
            queued.send(()).unwrap();
            let (first, second) = zip(first, second).await;
            first.unwrap();
            second.unwrap();
        });
        zip(
            async {
                waiting.await.unwrap();
                assert_eq!(state.get(), 1);
                release.send(()).unwrap();
                task.await.unwrap();
                files.close();
            },
            files.run(),
        )
        .await;
        assert_eq!(state.get(), 3);
        assert_eq!(Rc::strong_count(&state), 1);
    });
}
