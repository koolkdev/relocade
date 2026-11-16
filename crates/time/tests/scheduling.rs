use relocade_time::{Schedule, TickRate, Timestamp};
use std::{num::NonZeroU64, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn periodic_deadlines_keep_phase_across_wrap_and_late_observations() {
    let rate = TickRate::new(NonZeroU64::new(10).unwrap());
    let start = Timestamp::from_ticks(u64::MAX - 1);
    let mut schedule = Schedule::new(rate, start);
    for ticks in [1, 3, 6, 8] {
        schedule.advance(Duration::from_millis(250));
        assert_eq!(schedule.deadline(), Timestamp::from_ticks(ticks));
    }
    assert_eq!(
        schedule.elapsed_until(Timestamp::from_ticks(9)),
        Duration::from_millis(100)
    );
    assert_eq!(
        schedule.elapsed_until(Timestamp::from_ticks(7)),
        Duration::ZERO
    );
    schedule.reset(Timestamp::from_ticks(1));
    schedule.advance(Duration::from_millis(250));
    assert_eq!(schedule.deadline(), Timestamp::from_ticks(4));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn deadlines_round_forward_and_measurements_round_down() {
    for hz in [3, 3_000_000_000] {
        let rate = TickRate::new(NonZeroU64::new(hz).unwrap());
        let start = Timestamp::from_ticks(u64::MAX);
        let end = Timestamp::from_ticks(0);
        let nanos = if hz == 3 { 333_333_333 } else { 0 };
        assert_eq!(
            rate.duration_between(start, end),
            Duration::from_nanos(nanos)
        );
        assert_eq!(
            rate.duration_between_ceil(start, end),
            Duration::from_nanos(nanos + 1)
        );
        assert_eq!(rate.checked_duration_between(end, start), None);
        assert_eq!(rate.checked_deadline_after(start, Duration::MAX), None);
    }
    let rate = TickRate::new(NonZeroU64::new(3_000_000_000).unwrap());
    let start = Timestamp::from_ticks(1);
    assert_eq!(
        rate.deadline_after(start, Duration::from_nanos(1)),
        Timestamp::from_ticks(4)
    );
    assert_eq!(
        rate.deadline_before(start, Duration::from_nanos(1)),
        Timestamp::from_ticks(u64::MAX - 1)
    );
}
