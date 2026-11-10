use super::Wait;
use crate::worker::Thread;
use futures_channel::oneshot;
use std::{
    future::Future,
    pin::pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Wake, Waker},
    time::Duration,
};
use web_time::Instant;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test::wasm_bindgen_test]
async fn notifications_before_waiting_interrupt_timed_and_idle_waits() {
    let thread = Thread::new();
    thread
        .run(|| async {
            let mut wait = Wait::new();
            for deadline in [Some(Instant::now() + Duration::from_secs(3600)), None] {
                wait.waker().wake_by_ref();
                wait.until(deadline).await;
            }
            // The retained tokens were consumed; an idle wait must now suspend.
            assert!(futures_lite::future::poll_once(wait.until(None))
                .await
                .is_none());
        })
        .await
        .unwrap();
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_registered_wait_resumes_from_another_thread() {
    let thread = Thread::new();
    let (send_waker, receive_waker) = oneshot::channel();
    let finished = thread.run(|| async move {
        let mut wait = Wait::new();
        let waker = wait.waker();
        let mut pending = pin!(wait.until(None));
        assert!(futures_lite::future::poll_once(pending.as_mut())
            .await
            .is_none());
        send_waker.send(waker).unwrap();
        pending.await;
    });
    receive_waker.await.unwrap().wake();
    finished.await.unwrap();
}

struct Task(AtomicBool);
impl Wake for Task {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn cancelling_an_idle_wait_releases_its_task_registration() {
    let thread = Thread::new();
    let escaped = thread
        .run(|| async {
            let mut wait = Wait::new();
            let escaped = wait.waker();
            let task = Arc::new(Task(AtomicBool::new(false)));
            let waker = Waker::from(task.clone());
            {
                let mut pending = pin!(wait.until(None));
                assert!(pending
                    .as_mut()
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending());
            }
            escaped.wake_by_ref();
            assert!(!task.0.load(Ordering::Relaxed));
            drop(waker);
            assert_eq!(Arc::strong_count(&task), 1);
            escaped
        })
        .await
        .unwrap();
    // An escaped notification owns no JS handles or completed worker task.
    escaped.wake();
}
