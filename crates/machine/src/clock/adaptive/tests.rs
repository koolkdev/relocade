use super::{AdaptiveTime, ClockProgress};
use std::time::Duration;

fn progress(work: u64, idle_us: u64) -> ClockProgress {
    ClockProgress {
        work,
        idle_time: Duration::from_micros(idle_us),
    }
}

fn calibrated() -> AdaptiveTime {
    let mut time = AdaptiveTime::new(Duration::ZERO, progress(0, 0));
    time.sample(Duration::from_micros(100), progress(10, 0));
    time
}

#[test]
fn uncalibrated_work_requires_a_host_sample_but_idle_time_still_advances() {
    let mut time = AdaptiveTime::new(Duration::from_micros(100), progress(50, 20));
    assert_eq!(
        time.estimate(progress(50, 70)),
        Some(Duration::from_micros(150))
    );
    assert_eq!(time.estimate(progress(51, 70)), None);
    // Fifty idle microseconds leave fifty active microseconds for ten units.
    time.sample(Duration::from_micros(200), progress(60, 70));
    assert_eq!(
        time.estimate(progress(70, 70)),
        Some(Duration::from_micros(250))
    );
}

#[test]
fn mixed_execution_and_idle_time_do_not_change_a_stable_execution_rate() {
    let mut time = calibrated();
    // A second ten-unit execution takes 100 us, followed by 50 ms of idle time.
    assert_eq!(
        time.sample(Duration::from_micros(50_200), progress(20, 50_000)),
        Duration::from_micros(50_200)
    );
    assert_eq!(
        time.estimate(progress(30, 50_000)),
        Some(Duration::from_micros(50_300))
    );
    // Another millisecond of idle time advances the timeline directly.
    assert_eq!(
        time.estimate(progress(30, 51_000)),
        Some(Duration::from_micros(51_300))
    );
}

#[test]
fn idle_only_samples_preserve_the_learned_rate() {
    let mut time = calibrated();
    time.sample(Duration::from_micros(1_000_100), progress(10, 1_000_000));
    assert_eq!(
        time.estimate(progress(20, 1_000_000)),
        Some(Duration::from_micros(1_000_200))
    );
}

#[test]
fn workload_changes_adjust_the_rate_gradually() {
    let mut time = calibrated();
    // The next ten units take 200 us: the rate moves from 10 to 12.5 us/unit.
    time.sample(Duration::from_micros(300), progress(20, 0));
    assert_eq!(
        time.estimate(progress(24, 0)),
        Some(Duration::from_micros(350))
    );
}

#[test]
fn resampling_corrects_ahead_estimates_without_rewinding_or_compounding_them() {
    let mut time = calibrated();
    assert_eq!(
        time.estimate(progress(20, 0)),
        Some(Duration::from_micros(200))
    );
    assert_eq!(
        time.sample(Duration::from_micros(110), progress(20, 0)),
        Duration::from_micros(200)
    );
    assert_eq!(
        time.estimate(progress(21, 0)),
        Some(Duration::from_micros(200))
    );
    assert_eq!(
        time.sample(Duration::from_micros(120), progress(30, 0)),
        Duration::from_micros(200)
    );
    assert_eq!(
        time.sample(Duration::from_micros(250), progress(40, 0)),
        Duration::from_micros(250)
    );
}

#[test]
fn a_sample_does_not_publish_an_unobserved_prediction() {
    let mut time = calibrated();
    // The old rate would predict 1 ms, but nobody observed that prediction.
    assert_eq!(
        time.sample(Duration::from_micros(110), progress(100, 0)),
        Duration::from_micros(110)
    );
}

#[test]
fn equal_host_timestamps_preserve_work_for_calibration() {
    let mut time = AdaptiveTime::new(Duration::ZERO, progress(0, 0));
    time.sample(Duration::ZERO, progress(1, 0));
    time.sample(Duration::ZERO, progress(2, 0));
    assert_eq!(time.estimate(progress(3, 0)), None);
    time.sample(Duration::from_nanos(1000), progress(3, 0));
    assert_eq!(
        time.estimate(progress(6, 0)),
        Some(Duration::from_nanos(2000))
    );
}

#[test]
fn equal_timestamps_reset_predictions_while_preserving_rate_calibration() {
    let mut time = calibrated();
    assert_eq!(
        time.sample(Duration::from_micros(100), progress(20, 0)),
        Duration::from_micros(100)
    );
    assert_eq!(
        time.estimate(progress(20, 0)),
        Some(Duration::from_micros(100))
    );
    // Twenty units occurred between distinct timestamps: 5 us/unit. The prior
    // 10 us/unit estimate moves to 8.75 us/unit, retaining the quantized work.
    time.sample(Duration::from_micros(200), progress(30, 0));
    assert_eq!(
        time.estimate(progress(34, 0)),
        Some(Duration::from_micros(235))
    );
}

#[test]
fn observations_without_measurable_execution_time_do_not_replace_the_rate() {
    let mut time = calibrated();
    // A zero-work observation cannot establish the duration of a unit of work.
    time.sample(Duration::from_micros(200), progress(10, 0));
    assert_eq!(
        time.estimate(progress(11, 0)),
        Some(Duration::from_micros(210))
    );
    // Quantization can leave no positive active duration after subtracting idle.
    time.sample(Duration::from_micros(300), progress(20, 101));
    assert_eq!(
        time.estimate(progress(21, 101)),
        Some(Duration::from_micros(310))
    );
}

#[test]
fn frequent_estimates_do_not_accumulate_fractional_rounding_error() {
    let mut time = AdaptiveTime::new(Duration::ZERO, progress(0, 0));
    time.sample(Duration::from_nanos(1), progress(3, 0));
    for work in 4..3003 {
        time.estimate(progress(work, 0));
    }
    assert_eq!(
        time.estimate(progress(3003, 0)),
        Some(Duration::from_nanos(1001))
    );
}

#[test]
fn execution_counter_wrap_preserves_progress() {
    let mut time = AdaptiveTime::new(Duration::ZERO, progress(u64::MAX - 1, 0));
    time.sample(Duration::from_nanos(2), progress(0, 0));
    assert_eq!(time.estimate(progress(3, 0)), Some(Duration::from_nanos(5)));
}
