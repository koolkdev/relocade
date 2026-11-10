use super::*;
use crate::GuestTimeDriver;
use std::num::NonZeroU64;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn correspondence_rounds_deadlines_forward_and_preserves_fractional_pauses() {
    let driver = GuestTimeDriver::new(NonZeroU64::new(3).unwrap());
    driver.advance_to(Timestamp::from_ticks(10));
    let mut host = HostTime::new(driver.clock(), Timebase(NonZeroU64::new(7).unwrap()));
    host.resume(Timestamp::from_ticks(20));

    // Guest offsets of +1/7 ns and -1/7 ns both round toward the future.
    assert_eq!(
        host.host_time(Timestamp::from_ticks(21)),
        Some(Timestamp::from_ticks(13))
    );
    assert_eq!(
        host.host_time(Timestamp::from_ticks(19)),
        Some(Timestamp::from_ticks(10))
    );
    assert_eq!(
        host.host_time(Timestamp::from_ticks(13)),
        Some(Timestamp::from_ticks(7))
    );
    assert_eq!(host.sync_deadline(None), Some(Timestamp::from_ticks(20)));
    driver.advance_to(Timestamp::from_ticks(11));
    assert_eq!(host.sync_deadline(None), Some(Timestamp::from_ticks(27)));

    // Two separate 1/3 ns pauses shift the host anchor by exactly two ticks.
    host.pause();
    driver.advance_to(Timestamp::from_ticks(12));
    host.resume(Timestamp::from_ticks(20));
    driver.advance_to(Timestamp::from_ticks(13));
    host.pause();
    driver.advance_to(Timestamp::from_ticks(14));
    host.resume(Timestamp::from_ticks(20));
    assert_eq!(
        host.host_time(Timestamp::from_ticks(20)),
        Some(Timestamp::from_ticks(12))
    );
    assert_eq!(host.sync_deadline(None), Some(Timestamp::from_ticks(27)));
}
