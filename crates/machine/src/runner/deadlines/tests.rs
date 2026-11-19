use super::*;
use crate::GuestTimeConfig;
use std::num::NonZeroU64;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn reprogramming_replaces_the_hardware_target_while_ordinary_sleeps_remain_live() {
    let schedule = Rc::new(ExecutionSchedule::with_guest_time(GuestTimeConfig::new(
        NonZeroU64::new(1000).unwrap(),
    )));
    let driver = schedule.guest_driver().unwrap();
    let mut deadlines = DeviceDeadlines::default();
    let mut cx = Context::from_waker(Waker::noop());
    let at = Timestamp::from_ticks;
    let mut sleep = schedule.clock().sleep_until(at(10));
    assert!(Pin::new(&mut sleep).poll(&mut cx).is_pending());
    let timer = deadlines.register(&schedule, Some(at(15)), |_| panic!("cancelled timer"));
    deadlines.poll(&mut cx);
    assert_eq!(driver.next_deadline(), Some(at(10)));
    timer.set(Some(at(3)));
    deadlines.poll(&mut cx);
    assert_eq!(driver.next_deadline(), Some(at(3)));
    timer.set(None);
    deadlines.poll(&mut cx);
    assert_eq!(driver.next_deadline(), Some(at(10)));

    let calls = Rc::new(Cell::new(0));
    for _ in 0..2 {
        let calls = calls.clone();
        deadlines.register(&schedule, Some(at(4)), move |now| {
            assert_eq!(now, Timestamp::from_ticks(4));
            calls.set(calls.get() + 1);
            Some(Timestamp::from_ticks(6))
        });
    }
    deadlines.poll(&mut cx);
    assert!(driver.advance_to_next_deadline());
    deadlines.poll(&mut cx);
    assert_eq!(calls.get(), 2);
    assert_eq!(driver.next_deadline(), Some(at(6)));
    drop(deadlines);
    assert_eq!(driver.next_deadline(), Some(at(10)));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn external_clock_wait_moves_to_the_next_enabled_source() {
    let clock = crate::GuestTimeDriver::new(NonZeroU64::new(1000).unwrap());
    let schedule = Rc::new(ExecutionSchedule::from_clock(clock.clock()));
    let mut deadlines = DeviceDeadlines::default();
    let at = Timestamp::from_ticks;
    let timer = deadlines.register(&schedule, Some(at(12)), |_| panic!("disarmed timer"));
    let _second = deadlines.register(&schedule, Some(at(20)), |now| {
        assert_eq!(now, Timestamp::from_ticks(20));
        None
    });
    let mut cx = Context::from_waker(Waker::noop());
    deadlines.poll(&mut cx);
    assert_eq!(clock.next_deadline(), Some(at(12)));
    timer.set(None);
    deadlines.poll(&mut cx);
    assert_eq!(clock.next_deadline(), Some(at(20)));
    assert!(clock.advance_to_next_deadline());
    deadlines.poll(&mut cx);
    assert_eq!(clock.next_deadline(), None);
}
