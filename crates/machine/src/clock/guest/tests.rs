use super::*;
use crate::{ClockCursor, ClockSource, Instant, Sleep};
use std::{
    cell::Cell,
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

struct HostClock {
    origin: Instant,
    time: Rc<Cell<Instant>>,
}

impl ClockSource for HostClock {
    fn now(&self) -> Timestamp {
        self.host_timestamp(self.time.get()).unwrap()
    }

    fn host_timestamp(&self, time: Instant) -> Option<Timestamp> {
        Some(Timestamp::from_ticks(
            time.saturating_duration_since(self.origin).as_nanos() as u64,
        ))
    }

    fn sleep_until(&self, _: Timestamp) -> Sleep {
        panic!("guest wait checks do not poll host timers")
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mapped_waits_follow_repeated_recovery_but_exclude_pauses() {
    let host_start = Instant::now();
    let host = Rc::new(Cell::new(host_start));
    let driver = GuestTimeDriver::new(NonZeroU64::new(1000).unwrap()).with_host_clock(Clock::new(
        HostClock {
            origin: host_start,
            time: host.clone(),
        },
    ));
    let clock = driver.clock();
    let guest_start = clock.now();
    driver
        .0
        .borrow_mut()
        .host
        .as_mut()
        .unwrap()
        .resume(guest_start);
    let clock = driver.clock();
    let mut cx = Context::from_waker(Waker::noop());
    let mut input = pin!(clock.sync_to(host_start + Duration::from_millis(100)));
    let older = clock.sync_to(host_start + Duration::from_millis(10));
    assert!(input.as_mut().poll(&mut cx).is_pending());

    host.set(host.get() + Duration::from_millis(20));
    driver.synchronize(None);
    assert_eq!(
        driver.next_deadline(),
        Some(clock.add(guest_start, Duration::from_millis(100)))
    );

    host.set(host.get() + Duration::from_nanos(1));
    driver.synchronize(None);
    assert_eq!(
        driver.next_deadline(),
        Some(clock.add(guest_start, Duration::from_nanos(79_999_999)))
    );
    assert!(pin!(older).poll(&mut cx).is_ready());

    driver.0.borrow_mut().host.as_mut().unwrap().pause();
    host.set(host.get() + Duration::from_secs(1));
    driver.synchronize(None);
    driver
        .0
        .borrow_mut()
        .host
        .as_mut()
        .unwrap()
        .resume(guest_start);
    assert_eq!(
        driver.next_deadline(),
        Some(clock.add(guest_start, Duration::from_nanos(79_999_999)))
    );
    let after_pause = clock.sync_to(host.get() + Duration::from_millis(60));

    // Another 31 ms of host time with 10 ms of guest progress discards 21 ms.
    driver.advance_to(clock.add(guest_start, Duration::from_millis(10)));
    host.set(host.get() + Duration::from_millis(31));
    driver.synchronize(None);
    assert_eq!(
        clock.now(),
        clock.add(guest_start, Duration::from_millis(10))
    );
    assert_eq!(
        driver.next_deadline(),
        Some(clock.add(guest_start, Duration::from_nanos(58_999_999)))
    );
    {
        let mut cancelled = pin!(after_pause);
        assert!(cancelled.as_mut().poll(&mut cx).is_pending());
        assert_eq!(
            driver.next_deadline(),
            Some(clock.add(guest_start, Duration::from_millis(39)))
        );
    }
    assert_eq!(
        driver.next_deadline(),
        Some(clock.add(guest_start, Duration::from_nanos(58_999_999)))
    );
    driver.advance_to(clock.add(guest_start, Duration::from_nanos(58_999_999)));
    assert!(input.as_mut().poll(&mut cx).is_ready());
    assert_eq!(driver.next_deadline(), None);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mapped_waits_retain_fractional_ticks_across_lag_recovery() {
    let origin = Instant::now();
    let host = Rc::new(Cell::new(origin));
    let driver = GuestTimeDriver::new(NonZeroU64::new(1000).unwrap()).with_host_clock(Clock::new(
        HostClock {
            origin,
            time: host.clone(),
        },
    ));
    let clock = driver.clock();
    let start = clock.now();
    driver.0.borrow_mut().host.as_mut().unwrap().resume(start);
    let mut wait = pin!(clock.sync_to(origin + Duration::from_millis(100)));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    // Each recovery removes 20.4 ticks. Rounding each subtraction would retain
    // an extra tick after the third recovery, delaying the input event.
    for tick in [80, 60, 39] {
        host.set(host.get() + Duration::from_micros(20_400));
        driver.synchronize(None);
        assert_eq!(driver.next_deadline(), Some(Timestamp::from_ticks(tick)));
    }
    assert!(driver.advance_to_next_deadline());
    assert!(wait.as_mut().poll(&mut cx).is_ready());
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
    let mut later = pin!(clock.sleep_until(clock.add(start, Duration::from_millis(20))));
    assert_eq!(driver.next_deadline(), None);
    assert!(later.as_mut().poll(&mut cx).is_pending());
    {
        let mut cancelled = pin!(clone.sleep_until(clock.add(start, Duration::from_millis(5))));
        assert!(cancelled.as_mut().poll(&mut cx).is_pending());
        assert_eq!(
            driver.next_deadline(),
            Some(clock.add(start, Duration::from_millis(5)))
        );
    }
    assert_eq!(
        driver.next_deadline(),
        Some(clock.add(start, Duration::from_millis(20)))
    );

    let mut first = pin!(clock.sleep_until(clock.add(start, Duration::from_millis(10))));
    let mut same = pin!(clone.sleep_until(clock.add(start, Duration::from_millis(10))));
    assert!(first.as_mut().poll(&mut cx).is_pending());
    assert!(same.as_mut().poll(&mut cx).is_pending());
    driver.advance_to(clock.add(start, Duration::from_millis(9)));
    assert_eq!(notifications.0.load(Ordering::Relaxed), 0);
    assert!(first.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        driver.next_deadline(),
        Some(clock.add(start, Duration::from_millis(10)))
    );
    driver.advance_to(clock.add(start, Duration::from_millis(10)));
    assert!(notifications.0.load(Ordering::Relaxed) > 0);
    assert!(first.as_mut().poll(&mut cx).is_ready());
    assert!(same.as_mut().poll(&mut cx).is_ready());
    assert!(later.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        driver.next_deadline(),
        Some(clock.add(start, Duration::from_millis(20)))
    );

    driver.advance_to(clock.add(start, Duration::from_millis(5)));
    assert_eq!(clone.now(), clock.add(start, Duration::from_millis(10)));
    let mut overdue = pin!(clock.sleep_until(clock.add(start, Duration::from_millis(5))));
    assert!(overdue.as_mut().poll(&mut cx).is_ready());
    assert_eq!(
        driver.next_deadline(),
        Some(clock.add(start, Duration::from_millis(20)))
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
    let deadline = clock.add(start, Duration::from_millis(250));
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
        clock.duration_since(clock.now(), start),
        Duration::from_millis(300)
    );
    assert_eq!(clock.sub(deadline, Duration::from_millis(300)), start);
    assert_eq!(clock.add(start, Duration::ZERO), start);
    assert!(clock.checked_add(start, Duration::MAX).is_none());
    assert!(clock.checked_sub(start, Duration::MAX).is_none());
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
fn elapsed_consumption_retains_fractions_at_coarse_and_fine_tick_rates() {
    for rate in [3, 3_000_000_000] {
        let driver = GuestTimeDriver::new(NonZeroU64::new(rate).unwrap());
        driver.advance_to(Timestamp::from_ticks(1));
        let clock = driver.clock();
        let start = clock.now();
        let mut cursor = ClockCursor::from(start);
        let mut total = Duration::ZERO;
        for reading in 2..=7 {
            driver.advance_to(Timestamp::from_ticks(reading));
            total += clock.consume_elapsed(&mut cursor, clock.now());
            assert_eq!(total, clock.duration_since(clock.now(), start));
        }
        let expected = if rate == 3 { 2_000_000_000 } else { 2 };
        assert_eq!(total, Duration::from_nanos(expected));
        assert_eq!(cursor, ClockCursor::from(Timestamp::from_ticks(7)));
        assert_eq!(clock.consume_elapsed(&mut cursor, start), Duration::ZERO);
        assert_eq!(cursor, ClockCursor::from(Timestamp::from_ticks(7)));
        assert_eq!(
            clock.checked_duration_since(start, Timestamp::from_ticks(7)),
            None
        );
    }
}
