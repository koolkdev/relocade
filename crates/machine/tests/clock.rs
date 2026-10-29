use futures_util::future::join;
use relocade_machine::{timer_service, Clock};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Waker},
    time::Duration,
};

async fn clock_waits() {
    let (clock, timers) = timer_service();
    let other = clock.clone();
    let waits = async move {
        let deadline = clock.now() + Duration::from_millis(10);
        let long = async {
            clock.wait_until(deadline).await;
            assert!(clock.now() >= deadline);
        };
        let short = async {
            let start = other.now();
            other.sleep(Duration::from_millis(1)).await;
            assert!(other.now() >= start + Duration::from_millis(1));
        };
        join(long, short).await;
    };
    join(timers.run(), waits).await;
}

async fn delayed_driver() {
    let (clock, timers) = timer_service();
    let mut driver = pin!(timers.run());
    let mut cx = Context::from_waker(Waker::noop());
    {
        let deadline = clock.now() + Duration::from_millis(1);
        let mut sleep = pin!(clock.wait_until(deadline));
        assert!(sleep.as_mut().poll(&mut cx).is_pending());
        // Delivery time is part of the wait, not an extension to its deadline.
        futures_timer::Delay::new(Duration::from_millis(10)).await;
        assert!(driver.as_mut().poll(&mut cx).is_pending());
        assert!(sleep.as_mut().poll(&mut cx).is_ready());
    }
    drop(clock);
    driver.await;
}

async fn cancelled_wait() {
    let (clock, timers) = timer_service();
    let mut driver = pin!(timers.run());
    let mut cx = Context::from_waker(Waker::noop());
    {
        let mut sleep = pin!(clock.sleep(Duration::from_secs(3600)));
        assert!(sleep.as_mut().poll(&mut cx).is_pending());
        assert!(driver.as_mut().poll(&mut cx).is_pending());
    }
    // Cancellation removes the active timer without waiting for its deadline.
    drop(clock);
    assert!(driver.as_mut().poll(&mut cx).is_ready());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shared_deadlines_and_relative_sleeps() {
    pollster::block_on(clock_waits());
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn queued_time_counts_toward_the_deadline() {
    pollster::block_on(delayed_driver());
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cancelling_a_wait_releases_its_timer_and_driver() {
    pollster::block_on(cancelled_wait());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn shared_deadlines_and_relative_sleeps() {
    clock_waits().await;
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn queued_time_counts_toward_the_deadline() {
    delayed_driver().await;
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn cancelling_a_wait_releases_its_timer_and_driver() {
    cancelled_wait().await;
}
