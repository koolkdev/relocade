use super::*;
use crate::{ClockSource, Instant, Sleep};
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
            time.saturating_duration_since(self.origin).as_nanos(),
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
