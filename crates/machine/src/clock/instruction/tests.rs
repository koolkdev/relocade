use super::*;
use relocade_clock_test_support::origin as clock_origin;
use std::{
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::Wake,
};

#[derive(Default)]
struct Notifications(AtomicUsize);

impl Wake for Notifications {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn deadline_subscription_wakes_and_releases_its_task() {
    let clock = InstructionTimeline::new(clock_origin(), NonZeroU64::new(1000).unwrap());
    let mut idle = Context::from_waker(Waker::noop());
    let mut later = Box::pin(
        clock
            .clock()
            .sleep_until(clock_origin() + Duration::from_millis(20)),
    );
    assert!(later.as_mut().poll(&mut idle).is_pending());

    let task = Arc::new(Notifications::default());
    let retained = Arc::downgrade(&task);
    let waker = Waker::from(task.clone());
    let mut cx = Context::from_waker(&waker);
    clock.watch_deadlines(&mut cx);

    let mut earlier = Box::pin(
        clock
            .clock()
            .sleep_until(clock_origin() + Duration::from_millis(10)),
    );
    assert!(earlier.as_mut().poll(&mut idle).is_pending());
    assert!(task.0.swap(0, Ordering::Relaxed) > 0);
    clock.watch_deadlines(&mut cx);
    drop(earlier);
    assert!(task.0.load(Ordering::Relaxed) > 0);
    assert_eq!(
        clock.next_deadline(),
        Some(clock_origin() + Duration::from_millis(20))
    );

    clock.unwatch_deadlines();
    drop(waker);
    drop(task);
    // Neither registration nor cancellation may retain the completed run,
    // even though the device's later wait survives for a replacement run.
    assert!(retained.upgrade().is_none());
    drop(later);
    assert_eq!(clock.next_deadline(), None);
}
