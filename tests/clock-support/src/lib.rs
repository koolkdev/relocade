use futures_channel::oneshot;
use relocade_machine::{Clock, ClockSource, Instant, Sleep, Timestamp};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

/// Explicitly controlled time for tests. Clones share progress;
/// separate instances can control host and guest time independently. The controller
/// can move between workers, while each consumer's [`Clock`] remains local.
#[derive(Clone, Default)]
pub struct ManualClock(Arc<Mutex<State>>);

struct State {
    origin: Instant,
    now: Timestamp,
    waits: Vec<Wait>,
}

struct Wait {
    deadline: Timestamp,
    complete: oneshot::Sender<()>,
}

impl ManualClock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clock(&self) -> Clock {
        Clock::new(self.clone())
    }

    /// Position a fixed duration after this controlled clock's start.
    pub fn at(&self, duration: Duration) -> Timestamp {
        self.clock().add(ORIGIN, duration)
    }

    pub fn elapsed(&self, time: Timestamp) -> Duration {
        self.clock().duration_since(time, ORIGIN)
    }

    pub fn advance_by(&self, duration: Duration) {
        let due = {
            let mut state = self.0.lock().unwrap();
            let now = self.clock().add(state.now, duration);
            state.advance_to(now)
        };
        Self::notify(due);
    }

    /// Moves forward to `timestamp` and wakes due sleeps. An earlier timestamp
    /// leaves time unchanged. Wakers run after releasing the clock state.
    pub fn advance_to(&self, timestamp: Timestamp) {
        let due = self.0.lock().unwrap().advance_to(timestamp);
        Self::notify(due);
    }

    /// External host instant corresponding to the controlled reading. Use this
    /// for timestamped input fixtures; device deadlines use the clock directly.
    pub fn host_now(&self) -> Instant {
        self.host_time(self.clock().now())
    }

    pub fn host_time(&self, time: Timestamp) -> Instant {
        self.0.lock().unwrap().origin + self.clock().duration_since(time, ORIGIN)
    }

    /// Earliest polled sleep, excluding cancelled waits.
    pub fn next_deadline(&self) -> Option<Timestamp> {
        let mut state = self.0.lock().unwrap();
        state.waits.retain(|wait| !wait.complete.is_canceled());
        state.waits.iter().map(|wait| wait.deadline).min()
    }

    fn notify(due: Vec<Wait>) {
        for wait in due {
            let _ = wait.complete.send(());
        }
    }
}

impl ClockSource for ManualClock {
    fn now(&self) -> Timestamp {
        self.0.lock().unwrap().now
    }

    fn host_timestamp(&self, time: Instant) -> Option<Timestamp> {
        let state = self.0.lock().unwrap();
        Some(Timestamp::from_ticks(
            time.saturating_duration_since(state.origin).as_nanos(),
        ))
    }

    fn sleep_until(&self, deadline: Timestamp) -> Sleep {
        let controller = self.clone();
        Sleep::new(async move {
            let completed = {
                let mut state = controller.0.lock().unwrap();
                if deadline <= state.now {
                    return;
                }
                state.waits.retain(|wait| !wait.complete.is_canceled());
                let (complete, completed) = oneshot::channel();
                state.waits.push(Wait { deadline, complete });
                completed
            };
            completed.await.expect("manual clock owns pending sleeps");
        })
    }
}

impl State {
    fn advance_to(&mut self, timestamp: Timestamp) -> Vec<Wait> {
        self.now = self.now.max(timestamp);
        self.waits
            .extract_if(.., |wait| {
                wait.deadline <= self.now || wait.complete.is_canceled()
            })
            .collect()
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            now: ORIGIN,
            waits: Vec::new(),
        }
    }
}

/// Initial coordinate of a controlled clock. Each fixture owns its timeline.
const ORIGIN: Timestamp = Timestamp::from_ticks(0);
