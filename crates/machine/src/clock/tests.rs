use super::{Clock, TimerHandle};
use futures_channel::mpsc;
use std::{
    future::Future,
    pin::pin,
    task::{Context, Waker},
    time::Duration,
};
use web_time::Instant;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn early_notification_preserves_the_deadline_and_cancellation() {
    let (requests, mut received) = mpsc::unbounded();
    let clock = TimerHandle {
        start: Instant::now(),
        requests,
    };
    let deadline = Duration::from_secs(3600);
    let mut cx = Context::from_waker(Waker::noop());
    let rearmed = {
        let mut wait = pin!(clock.wait_until(deadline));
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        let first = received.try_recv().unwrap();
        // Deliver the host notification while the caller is still before its deadline.
        first.complete.send(()).unwrap();
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        let rearmed = received.try_recv().unwrap();
        assert_eq!(rearmed.deadline, first.deadline);
        rearmed
    };
    assert!(rearmed.complete.is_canceled());
}
