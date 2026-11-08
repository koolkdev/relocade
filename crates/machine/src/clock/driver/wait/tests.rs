use super::Wait;
use std::time::Duration;
use web_time::Instant;

#[test]
fn notifications_before_waiting_interrupt_both_timed_and_idle_waits() {
    let mut wait = Wait::new();
    for deadline in [Some(Instant::now() + Duration::from_secs(3600)), None] {
        wait.waker().wake_by_ref();
        pollster::block_on(wait.until(deadline));
    }
}
