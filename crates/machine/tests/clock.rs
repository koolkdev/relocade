use futures_util::future::join;
use relocade_machine::{timer_service, Clock};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Waker},
    time::Duration,
};

async fn clock_waits() {
    let (handle, timers) = timer_service();
    let clock = Clock::from(handle);
    let other = clock.clone();
    let waits = async move {
        let deadline = clock.add(clock.now(), Duration::from_millis(10));
        let output = clock.sync_to_host(deadline);
        let long = async {
            output.await;
            assert!(clock.now() >= deadline);
        };
        let short = async {
            let start = other.now();
            other.sleep(Duration::from_millis(1)).await;
            assert!(other.now() >= other.add(start, Duration::from_millis(1)));
        };
        join(long, short).await;
    };
    join(timers.run(), waits).await;
}

async fn delayed_driver() {
    let (handle, timers) = timer_service();
    let clock = Clock::from(handle);
    let mut cx = Context::from_waker(Waker::noop());
    let worker;
    {
        let deadline = clock.add(clock.now(), Duration::from_millis(1));
        let mut sleep = pin!(clock.sleep_until(deadline));
        assert!(sleep.as_mut().poll(&mut cx).is_pending());
        // Delivery time is part of the wait, not an extension to its deadline.
        futures_timer::Delay::new(Duration::from_millis(10)).await;
        worker = timers.spawn();
        // A later request observes the worker processing the already overdue one.
        clock.sleep(Duration::ZERO).await;
        assert!(sleep.as_mut().poll(&mut cx).is_ready());
    }
    drop(clock);
    worker.join().await.unwrap();
}

async fn cancelled_wait() {
    let (handle, timers) = timer_service();
    let clock = Clock::from(handle);
    let worker = timers.spawn();
    let mut cx = Context::from_waker(Waker::noop());
    {
        let mut sleep = pin!(clock.sleep(Duration::from_secs(3600)));
        assert!(sleep.as_mut().poll(&mut cx).is_pending());
        clock.sleep(Duration::ZERO).await;
    }
    // Cancellation removes the active timer without waiting for its deadline.
    drop(clock);
    worker.join().await.unwrap();
}

async fn earlier_deadline() {
    let (handle, timers) = timer_service();
    let clock = Clock::from(handle);
    let worker = timers.spawn();
    {
        let mut later = pin!(clock.sleep(Duration::from_secs(3600)));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(later.as_mut().poll(&mut cx).is_pending());
        clock.sleep(Duration::ZERO).await;
        // Let the timer thread park on the long deadline before inserting a new one.
        futures_timer::Delay::new(Duration::from_millis(10)).await;
        let deadline = clock.add(clock.now(), Duration::from_micros(500));
        clock.sleep_until(deadline).await;
        assert!(clock.now() >= deadline);
        assert!(later.as_mut().poll(&mut cx).is_pending());
    }
    drop(clock);
    worker.join().await.unwrap();
}

async fn idle_worker() {
    let (handle, timers) = timer_service();
    let clock = Clock::from(handle);
    let worker = timers.spawn();
    // Completing the only request leaves the worker parked without a deadline.
    clock.sleep(Duration::ZERO).await;
    futures_timer::Delay::new(Duration::from_millis(10)).await;
    clock.sleep(Duration::from_millis(1)).await;
    // Explicit stop does not depend on releasing every cloned clock handle.
    worker.stop().await.unwrap();
    drop(clock);
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

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_new_earlier_deadline_interrupts_the_current_wait() {
    pollster::block_on(earlier_deadline());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_new_earlier_deadline_interrupts_the_current_wait() {
    earlier_deadline().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_idle_worker_accepts_requests_and_stops_with_surviving_handles() {
    pollster::block_on(idle_worker());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn an_idle_worker_accepts_requests_and_stops_with_surviving_handles() {
    idle_worker().await;
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn host_clock_maps_external_events_to_its_own_timeline() {
    use relocade_machine::{ClockSource, Instant};
    let before = Instant::now();
    let (handle, _driver) = timer_service();
    let clock = handle.clock();
    let now = clock.now();
    let after = Instant::now();
    assert!(handle.host_timestamp(before).unwrap() <= now);
    assert!(now <= handle.host_timestamp(after).unwrap());
    assert_eq!(
        handle.host_timestamp(before),
        Some(relocade_machine::Timestamp::from_ticks(0))
    );
}
