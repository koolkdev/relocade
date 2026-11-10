use futures_channel::oneshot;
use relocade_machine::{Clock, ClockSource, Instant, Sleep};
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
    now: Instant,
    waits: Vec<Wait>,
}

struct Wait {
    deadline: Instant,
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
    pub fn at(&self, duration: Duration) -> Instant {
        self.0.lock().unwrap().origin + duration
    }

    pub fn elapsed(&self, time: Instant) -> Duration {
        time.saturating_duration_since(self.0.lock().unwrap().origin)
    }

    pub fn advance_by(&self, duration: Duration) {
        let due = {
            let mut state = self.0.lock().unwrap();
            let now = state.now + duration;
            state.advance_to(now)
        };
        Self::notify(due);
    }

    /// Moves forward to `timestamp` and wakes due sleeps. An earlier timestamp
    /// leaves time unchanged. Wakers run after releasing the clock state.
    pub fn advance_to(&self, timestamp: Instant) {
        let due = self.0.lock().unwrap().advance_to(timestamp);
        Self::notify(due);
    }

    /// Earliest polled sleep, excluding cancelled waits.
    pub fn next_deadline(&self) -> Option<Instant> {
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
    fn now(&self) -> Instant {
        self.0.lock().unwrap().now
    }

    fn sleep_until(&self, deadline: Instant) -> Sleep {
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
    fn advance_to(&mut self, timestamp: Instant) -> Vec<Wait> {
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
        let origin = Instant::now();
        Self {
            origin,
            now: origin,
            waits: Vec::new(),
        }
    }
}
