use super::*;
use std::{
    future::Future,
    pin::pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Wake},
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
fn deadlines_follow_registration_cancellation_and_idle_time() {
    let start = Instant::now();
    let driver = GuestTimeDriver::new(start, NonZeroU64::new(1000).unwrap());
    let clock = driver.clock();
    let clone = clock.clone();
    let notifications = Arc::new(Notifications::default());
    let waker = Waker::from(notifications.clone());
    let mut cx = Context::from_waker(&waker);
    let mut later = pin!(clock.sleep_until(start + Duration::from_millis(20)));
    assert_eq!(driver.next_deadline(), None);
    assert!(later.as_mut().poll(&mut cx).is_pending());
    {
        let mut cancelled = pin!(clone.sleep_until(start + Duration::from_millis(5)));
        assert!(cancelled.as_mut().poll(&mut cx).is_pending());
        assert_eq!(
            driver.next_deadline(),
            Some(start + Duration::from_millis(5))
        );
    }
    assert_eq!(
        driver.next_deadline(),
        Some(start + Duration::from_millis(20))
    );

    let mut first = pin!(clock.sleep_until(start + Duration::from_millis(10)));
    let mut same = pin!(clone.sleep_until(start + Duration::from_millis(10)));
    assert!(first.as_mut().poll(&mut cx).is_pending());
    assert!(same.as_mut().poll(&mut cx).is_pending());
    driver.advance_to(start + Duration::from_millis(9));
    assert_eq!(notifications.0.load(Ordering::Relaxed), 0);
    assert!(first.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        driver.next_deadline(),
        Some(start + Duration::from_millis(10))
    );
    driver.advance_to(start + Duration::from_millis(10));
    assert!(notifications.0.load(Ordering::Relaxed) > 0);
    assert!(first.as_mut().poll(&mut cx).is_ready());
    assert!(same.as_mut().poll(&mut cx).is_ready());
    assert!(later.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        driver.next_deadline(),
        Some(start + Duration::from_millis(20))
    );

    driver.advance_to(start + Duration::from_millis(5));
    assert_eq!(clone.now(), start + Duration::from_millis(10));
    let mut overdue = pin!(clock.sleep_until(start + Duration::from_millis(5)));
    assert!(overdue.as_mut().poll(&mut cx).is_ready());
    assert_eq!(
        driver.next_deadline(),
        Some(start + Duration::from_millis(20))
    );
    assert!(driver.advance_to_next_deadline());
    assert!(later.as_mut().poll(&mut cx).is_ready());
    assert_eq!(driver.next_deadline(), None);
    assert!(!driver.advance_to_next_deadline());
}
