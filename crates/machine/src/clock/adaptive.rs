//! Adaptive time estimation from execution progress and occasional host samples.

use std::time::Duration;

/// Cumulative inputs to an adaptive time estimate.
///
/// `work` counts execution units chosen by the execution owner; it is not a claim
/// about physical CPU cycles. The counter may wrap, but must not be reset between
/// samples. `idle_time` counts elapsed time while guest execution was inactive,
/// including service waits and host/device work. It must not overlap execution
/// time or decrease. Both counters retain the same origin throughout the session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClockProgress {
    pub work: u64,
    pub idle_time: Duration,
}

/// Estimates elapsed time between host-clock samples using the observed work rate.
///
/// This is the arithmetic model for an adaptive clock, not a [`super::Clock`]
/// implementation. The execution owner supplies progress and host samples; this
/// type does not read a host clock, schedule waits or choose when to sample.
/// Work accounting and CPU/worker integration are separate responsibilities.
///
/// Host samples anchor the timeline. Idle time advances the estimate directly
/// and is excluded from rate calibration. Samples smooth the observed time per
/// work unit, with each new measurement contributing one quarter of the rate.
/// Returned timestamps never decrease, even when a host sample corrects an
/// estimate that ran ahead. Freezing time during a pause is a separate policy.
pub struct AdaptiveTime {
    anchor: Sample,
    // Progress at the last distinct host timestamp. Equal timestamps reset the
    // prediction anchor while retaining this interval's work for calibration.
    rate_progress: ClockProgress,
    seconds_per_work: Option<f64>,
    last_returned: Duration,
}

struct Sample {
    host_time: Duration,
    progress: ClockProgress,
}

impl AdaptiveTime {
    /// Starts at an observed host time and its corresponding execution progress.
    /// A later sample with work and positive execution time establishes the rate.
    pub fn new(host_time: Duration, progress: ClockProgress) -> Self {
        Self {
            anchor: Sample {
                host_time,
                progress,
            },
            rate_progress: progress,
            seconds_per_work: None,
            last_returned: host_time,
        }
    }

    /// Observes the host clock and recalibrates using only time spent executing.
    /// Zero-work and zero-execution-time observations retain the previous rate.
    /// Host time and cumulative idle time must not decrease.
    pub fn sample(&mut self, host_time: Duration, progress: ClockProgress) -> Duration {
        assert!(
            host_time >= self.anchor.host_time,
            "host time moved backwards"
        );
        self.check_progress(progress);
        let elapsed = host_time - self.anchor.host_time;
        if !elapsed.is_zero() {
            // Coarsened host timestamps can leave no measurable execution time.
            let idle = progress.idle_time - self.rate_progress.idle_time;
            let active = elapsed.saturating_sub(idle);
            let work = progress.work.wrapping_sub(self.rate_progress.work);
            if work != 0 && !active.is_zero() {
                let observed = active.as_secs_f64() / work as f64;
                self.seconds_per_work = Some(match self.seconds_per_work {
                    Some(previous) => previous + (observed - previous) * 0.25,
                    None => observed,
                });
            }
            self.rate_progress = progress;
        }
        // Re-anchor to actual samples, never to an estimate that ran ahead.
        self.anchor = Sample {
            host_time,
            progress,
        };
        self.last_returned = self.last_returned.max(host_time);
        self.last_returned
    }

    /// Estimates time without reading the host clock.
    ///
    /// Returns `None` when new work needs a rate that has not yet been learned;
    /// the caller must obtain another host sample. Idle-only progress can advance
    /// time before calibration. Calculating from the anchor avoids accumulating
    /// rounding error when the caller requests many small estimates.
    pub fn estimate(&mut self, progress: ClockProgress) -> Option<Duration> {
        self.check_progress(progress);
        let idle = progress.idle_time - self.anchor.progress.idle_time;
        let work = progress.work.wrapping_sub(self.anchor.progress.work);
        let active = if work == 0 {
            Duration::ZERO
        } else {
            Duration::try_from_secs_f64(work as f64 * self.seconds_per_work?)
                .unwrap_or(Duration::MAX)
        };
        let estimate = self
            .anchor
            .host_time
            .saturating_add(idle)
            .saturating_add(active);
        self.last_returned = self.last_returned.max(estimate);
        Some(self.last_returned)
    }

    fn check_progress(&self, progress: ClockProgress) {
        assert!(
            progress.idle_time >= self.anchor.progress.idle_time,
            "cumulative idle time moved backwards"
        );
    }
}

#[cfg(test)]
mod tests;
