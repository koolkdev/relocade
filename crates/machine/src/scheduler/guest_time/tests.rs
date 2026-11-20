use super::*;
use std::{
    future::Future,
    pin::pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Wake},
    time::Duration,
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
    let driver = GuestTimeDriver::new(NonZeroU64::new(1000).unwrap());
    let clock = driver.clock();
    let start = clock.now();
    let clone = clock.clone();
    let notifications = Arc::new(Notifications::default());
    let waker = Waker::from(notifications.clone());
    let mut cx = Context::from_waker(&waker);
    let mut later = pin!(clock.sleep_until(clock.deadline_after(start, Duration::from_millis(20))));
    assert_eq!(driver.next_deadline(), None);
    assert!(later.as_mut().poll(&mut cx).is_pending());
    {
        let mut cancelled =
            pin!(clone.sleep_until(clock.deadline_after(start, Duration::from_millis(5))));
        assert!(cancelled.as_mut().poll(&mut cx).is_pending());
        assert_eq!(
            driver.next_deadline(),
            Some(clock.deadline_after(start, Duration::from_millis(5)))
        );
    }
    assert_eq!(
        driver.next_deadline(),
        Some(clock.deadline_after(start, Duration::from_millis(20)))
    );

    let mut first = pin!(clock.sleep_until(clock.deadline_after(start, Duration::from_millis(10))));
    let mut same = pin!(clone.sleep_until(clock.deadline_after(start, Duration::from_millis(10))));
    assert!(first.as_mut().poll(&mut cx).is_pending());
    assert!(same.as_mut().poll(&mut cx).is_pending());
    driver.advance_to(clock.deadline_after(start, Duration::from_millis(9)));
    assert_eq!(notifications.0.load(Ordering::Relaxed), 0);
    let replacement = Arc::new(Notifications::default());
    let replacement_waker = Waker::from(replacement.clone());
    let mut replacement_cx = Context::from_waker(&replacement_waker);
    assert!(first.as_mut().poll(&mut replacement_cx).is_pending());
    assert_eq!(
        driver.next_deadline(),
        Some(clock.deadline_after(start, Duration::from_millis(10)))
    );
    driver.advance_to(clock.deadline_after(start, Duration::from_millis(10)));
    assert_eq!(notifications.0.load(Ordering::Relaxed), 1);
    assert_eq!(replacement.0.load(Ordering::Relaxed), 1);
    assert!(first.as_mut().poll(&mut cx).is_ready());
    assert!(same.as_mut().poll(&mut cx).is_ready());
    assert!(later.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        driver.next_deadline(),
        Some(clock.deadline_after(start, Duration::from_millis(20)))
    );

    driver.advance_to(clock.deadline_after(start, Duration::from_millis(5)));
    assert_eq!(
        clone.now(),
        clock.deadline_after(start, Duration::from_millis(10))
    );
    let mut overdue =
        pin!(clock.sleep_until(clock.deadline_after(start, Duration::from_millis(5))));
    assert!(overdue.as_mut().poll(&mut cx).is_ready());
    assert_eq!(
        driver.next_deadline(),
        Some(clock.deadline_after(start, Duration::from_millis(20)))
    );
    assert!(driver.advance_to_next_deadline());
    assert!(later.as_mut().poll(&mut cx).is_ready());
    assert_eq!(driver.next_deadline(), None);
    assert!(!driver.advance_to_next_deadline());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn native_deadlines_and_elapsed_time_cross_tick_wrap() {
    let driver = GuestTimeDriver::new(NonZeroU64::new(10).unwrap());
    driver.0.borrow_mut().offset = u64::MAX - 1;
    let clock = driver.clock();
    let start = clock.now();
    let deadline = clock.deadline_after(start, Duration::from_millis(250));
    assert_eq!(deadline, Timestamp::from_ticks(1));
    assert!(deadline > start);
    let mut wait = pin!(clock.sleep_until(deadline));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    driver.advance_to(Timestamp::from_ticks(0));
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    driver.advance_to_next_deadline();
    assert!(wait.as_mut().poll(&mut cx).is_ready());
    assert_eq!(
        clock.duration_between(start, clock.now()),
        Duration::from_millis(300)
    );
    assert_eq!(
        clock.deadline_before(deadline, Duration::from_millis(300)),
        start
    );
    assert_eq!(clock.deadline_after(start, Duration::ZERO), start);
    assert!(clock.checked_deadline_after(start, Duration::MAX).is_none());
    assert!(clock
        .checked_deadline_before(start, Duration::MAX)
        .is_none());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn periodic_scheduling_rounds_deadlines_without_accumulating_error() {
    let driver = GuestTimeDriver::new(NonZeroU64::new(10).unwrap());
    let clock = driver.clock();
    let mut phase = clock.schedule(clock.now());
    let mut cx = Context::from_waker(Waker::noop());
    // Each period is 2.5 instructions. Rounding the period itself would give
    // 3, 6, 9, 12 and permanently slow the timer.
    for tick in [3, 5, 8, 10] {
        phase.advance(Duration::from_millis(250));
        assert_eq!(phase.deadline(), Timestamp::from_ticks(tick));
        let mut wait = pin!(clock.sleep_until(phase.deadline()));
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        driver.advance_to(Timestamp::from_ticks(tick - 1));
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        driver.advance_to_next_deadline();
        assert!(wait.as_mut().poll(&mut cx).is_ready());
    }
    assert_eq!(
        clock.duration_between(Timestamp::from_ticks(0), phase.deadline()),
        Duration::from_secs(1)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn recalibration_preserves_fractional_time_and_reprojects_the_earliest_wait() {
    let host = GuestTimeDriver::new(NonZeroU64::new(1_000_000).unwrap());
    let rate = TickRate::new(NonZeroU64::new(1_000_000).unwrap());
    let mut live = Realtime::new(host.clock(), rate);
    let origin = Timestamp::from_ticks(u64::MAX - 999);
    let start = u64::MAX - 10_000;
    live.start(origin, start);
    live.running = true;
    let deadline = rate.deadline_after(origin, Duration::from_millis(3));
    let old = live.estimate.target(deadline, start).unwrap();
    let count = start.wrapping_add(50_003);
    let phase = live.estimate.phase(count);
    assert_ne!(phase % super::interpolation::ONE_TICK, 0);
    host.advance_to(Timestamp::from_ticks(4_000));
    live.sample(count, true);
    assert_eq!(live.estimate.phase(count), phase);
    let earlier = live.estimate.target(deadline, count).unwrap();
    assert!(earlier.wrapping_sub(count) < old.wrapping_sub(count));
    assert!(live.estimate.now(earlier - 1) < deadline);
    assert_eq!(live.estimate.now(earlier), deadline);
    assert_eq!(
        live.estimate.now(count.wrapping_add(50_003)),
        rate.deadline_after(origin, Duration::from_millis(8))
    );
    // Faster retirement cannot project past the end of this observation window.
    assert_eq!(
        live.estimate.now(count.wrapping_add(100_006)),
        rate.deadline_after(origin, Duration::from_millis(8))
    );
}
