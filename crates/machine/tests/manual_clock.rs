use relocade_clock_test_support::{origin as clock_origin, ManualClock};
use std::{
    future::Future,
    pin::pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Wake, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(Default)]
struct Notifications(AtomicUsize);

impl Wake for Notifications {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn owned_sleep_captures_its_deadline_before_polling_and_keeps_timebases_independent() {
    let host = ManualClock::new();
    let guest = ManualClock::new();
    let clock = guest.clock();
    let mut sleep = pin!(clock.sleep(Duration::from_millis(10)));
    drop(clock);
    guest.advance_by(Duration::from_millis(3));
    assert_eq!(guest.next_deadline(), None);
    let notifications = Arc::new(Notifications::default());
    let waker = Waker::from(notifications.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(sleep.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        guest.next_deadline(),
        Some(clock_origin() + Duration::from_millis(10))
    );

    host.advance_by(Duration::from_secs(1));
    assert_eq!(notifications.0.load(Ordering::Relaxed), 0);
    assert!(sleep.as_mut().poll(&mut cx).is_pending());
    guest.advance_by(Duration::from_millis(7));
    assert_eq!(notifications.0.load(Ordering::Relaxed), 1);
    assert!(sleep.as_mut().poll(&mut cx).is_ready());
    assert_eq!(guest.next_deadline(), None);
    assert_eq!(
        guest.clock().now() - clock_origin(),
        Duration::from_millis(10)
    );
    assert_eq!(host.clock().now() - clock_origin(), Duration::from_secs(1));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cancellation_and_repolling_preserve_the_remaining_deadlines_and_waiters() {
    let manual = ManualClock::new();
    let clock = manual.clock();
    let clone = clock.clone();
    let first = clock_origin() + Duration::from_millis(5);
    let later = clock_origin() + Duration::from_millis(10);
    let mut cancelled = Box::pin(clock.sleep_until(first));
    let mut sleep = pin!(clock.sleep_until(later));
    let mut other = pin!(clone.sleep_until(later));
    let old = Arc::new(Notifications::default());
    let current = Arc::new(Notifications::default());
    let old_waker = Waker::from(old.clone());
    let current_waker = Waker::from(current.clone());
    assert!(cancelled
        .as_mut()
        .poll(&mut Context::from_waker(&old_waker))
        .is_pending());
    assert!(sleep
        .as_mut()
        .poll(&mut Context::from_waker(&old_waker))
        .is_pending());
    assert!(sleep
        .as_mut()
        .poll(&mut Context::from_waker(&current_waker))
        .is_pending());
    assert!(other
        .as_mut()
        .poll(&mut Context::from_waker(&current_waker))
        .is_pending());
    assert_eq!(manual.next_deadline(), Some(first));
    drop(cancelled);
    assert_eq!(manual.next_deadline(), Some(later));
    manual.advance_to(first);
    assert_eq!(current.0.load(Ordering::Relaxed), 0);
    manual.advance_to(later);
    assert_eq!(old.0.load(Ordering::Relaxed), 0);
    assert_eq!(current.0.load(Ordering::Relaxed), 2);
    let mut cx = Context::from_waker(&current_waker);
    assert!(sleep.as_mut().poll(&mut cx).is_ready());
    assert!(other.as_mut().poll(&mut cx).is_ready());
    assert!(pin!(clock.sleep_until(first))
        .as_mut()
        .poll(&mut cx)
        .is_ready());
    assert_eq!(manual.next_deadline(), None);
}
