use super::{Clock, Sleep, Source, Timestamp};
use futures_channel::oneshot;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

/// Explicitly controlled time for tests and simulations. Clones share progress;
/// separate instances can control host and guest time independently. The controller
/// can move between workers, while each consumer's [`Clock`] remains local.
#[derive(Clone, Default)]
pub struct ManualClock(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
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

    pub fn advance_by(&self, duration: Duration) {
        let due = {
            let mut state = self.0.lock().unwrap();
            let now = state.now.saturating_add(duration);
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

impl Source for ManualClock {
    fn now(&self) -> Timestamp {
        self.0.lock().unwrap().now
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
