use super::*;
use crate::ClockSource;

struct HostClock(Rc<Cell<Instant>>);

impl ClockSource for HostClock {
    fn now(&self) -> Instant {
        self.0.get()
    }

    fn sleep_until(&self, _: Instant) -> Sleep {
        panic!("mapping checks do not poll host timers")
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn captured_input_deadlines_follow_repeated_recovery_but_exclude_pauses() {
    let host_start = Instant::now();
    let host = Rc::new(Cell::new(host_start));
    let guest_start = host_start + Duration::from_secs(1);
    let mut time = HostTime::new(Clock::new(HostClock(host.clone())));
    time.resume(guest_start);
    let future = time
        .sync_deadline(Some(host_start + Duration::from_millis(100)))
        .unwrap();
    let older = time
        .sync_deadline(Some(host_start + Duration::from_millis(10)))
        .unwrap();

    host.set(host.get() + Duration::from_millis(20));
    time.discard_lag(guest_start);
    assert_eq!(time.host_time(guest_start), Some(host_start));
    assert_eq!(future.at(), guest_start + Duration::from_millis(100));

    host.set(host.get() + Duration::from_nanos(1));
    time.discard_lag(guest_start);
    assert_eq!(time.host_time(guest_start), Some(host.get()));
    assert_eq!(future.at(), guest_start + Duration::from_nanos(79_999_999));
    assert_eq!(older.at(), guest_start);

    time.pause();
    host.set(host.get() + Duration::from_secs(1));
    time.discard_lag(guest_start);
    time.resume(guest_start);
    assert_eq!(time.host_time(guest_start), Some(host.get()));
    assert_eq!(future.at(), guest_start + Duration::from_nanos(79_999_999));
    let after_pause = time
        .sync_deadline(Some(host.get() + Duration::from_millis(60)))
        .unwrap();
    assert_eq!(after_pause.at(), guest_start + Duration::from_millis(60));

    // Another 31 ms of host time with 10 ms of guest progress discards 21 ms.
    let guest_now = guest_start + Duration::from_millis(10);
    host.set(host.get() + Duration::from_millis(31));
    time.discard_lag(guest_now);
    assert_eq!(time.host_time(guest_now), Some(host.get()));
    assert_eq!(future.at(), guest_start + Duration::from_nanos(58_999_999));
    assert_eq!(after_pause.at(), guest_start + Duration::from_millis(39));
    assert_eq!(time.sync_deadline(None).unwrap().at(), guest_now);
}
