use super::*;
use std::{future::Future, pin::pin, task::Wake};

#[derive(Default)]
struct Wakes(AtomicUsize);

impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn pause_drains_active_operations_and_blocks_entry_until_release() {
    let execution = Arc::new(Execution::default());
    let first = execution.member();
    let active = first.try_enter().unwrap();
    let wakes = Arc::new(Wakes::default());
    let waker = wakes.clone().into();
    let cx = &mut Context::from_waker(&waker);
    let mut pause = pin!(execution.pause(|| {}));
    assert!(pause.as_mut().poll(cx).is_pending());
    assert!(first.try_enter().is_none());
    let late = execution.member();
    assert!(late.poll_enter(cx).is_pending());
    drop(active);
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    let Poll::Ready(paused) = pause.as_mut().poll(cx) else {
        panic!("last operation has finished");
    };
    assert!(first.try_enter().is_none());
    drop(paused);
    assert_eq!(wakes.0.load(Ordering::SeqCst), 2);
    assert!(late.poll_enter(cx).is_ready());
}

#[test]
fn cancelling_a_pause_reopens_entry_and_queued_pauses_wait_for_release() {
    let execution = Arc::new(Execution::default());
    let member = execution.member();
    let active = member.try_enter().unwrap();
    let cx = &mut Context::from_waker(std::task::Waker::noop());
    let mut cancelled = Box::pin(execution.pause(|| {}));
    assert!(cancelled.as_mut().poll(cx).is_pending());
    drop(cancelled);
    assert!(member.try_enter().is_some());
    drop(active);

    let mut first = pin!(execution.pause(|| {}));
    let Poll::Ready(first) = first.as_mut().poll(cx) else {
        panic!("no operations are active");
    };
    let mut second = pin!(execution.pause(|| {}));
    assert!(second.as_mut().poll(cx).is_pending());
    drop(first);
    let Poll::Ready(second) = second.as_mut().poll(cx) else {
        panic!("preceding pause was released");
    };
    assert!(member.try_enter().is_none());
    drop(second);
    assert!(member.try_enter().is_some());
}
