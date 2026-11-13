//! Integrates timestamped speaker changes into complete PCM sample intervals.

use crate::Speaker;
use relocade_machine::{Clock, Timestamp};
use relocade_pit::{Channel2State, INPUT_HZ};
use std::{cell::RefCell, collections::VecDeque, ops::Range, rc::Rc};

struct Signal {
    state: Channel2State,
    enabled: bool,
}

impl Signal {
    /// High time in billionths of one PIT input clock. Advancing while muted
    /// preserves phase; only the audible contribution becomes zero.
    /// Keep the cursor on the PIT's consumed nanosecond timeline so fractions
    /// of a nanosecond remain available for the next update.
    fn advance_to(&mut self, clock: &Clock, end: Timestamp) -> u128 {
        let elapsed = clock.consume_elapsed(&mut self.state.at, end);
        let span = elapsed.as_nanos() * u128::from(INPUT_HZ) + u128::from(self.state.fraction);
        let before = u128::from(self.state.counter.output());
        let high = self.state.counter.advance((span / 1_000_000_000) as u64);
        let fraction = (span % 1_000_000_000) as u32;
        let integral = u128::from(high) * 1_000_000_000
            + u128::from(fraction) * u128::from(self.state.counter.output())
            - u128::from(self.state.fraction) * before;
        self.state.fraction = fraction;
        if self.enabled {
            integral
        } else {
            0
        }
    }
}

/// Clocked mono samples of the speaker's digital drive, in the range 0..=1.
/// The consumer chooses sample intervals and playback gain.
pub struct AudioSource {
    clock: Clock,
    signal: Signal,
    changes: Rc<RefCell<VecDeque<Signal>>>,
}

impl Speaker {
    /// Records PIT and control writes for clocked audio rendering.
    /// Replaces any previous source; dropping it stops recording changes.
    pub fn audio_source(&self) -> AudioSource {
        let mut pit = self.pit.borrow_mut();
        let changes = Rc::new(RefCell::new(VecDeque::new()));
        let recorded = Rc::downgrade(&changes);
        let control = self.control.clone();
        pit.observe_channel2(move |state| {
            if let Some(changes) = recorded.upgrade() {
                changes.borrow_mut().push_back(Signal {
                    state,
                    enabled: control.get() & 2 != 0,
                });
            }
        });
        let signal = changes.borrow_mut().pop_front().unwrap();
        AudioSource {
            clock: pit.clock().clone(),
            signal,
            changes,
        }
    }
}

impl AudioSource {
    pub(super) fn started_at(&self) -> Timestamp {
        self.signal.state.at
    }

    /// Averages the drive over an elapsed interval on the PIT's clock.
    /// Intervals must progress forward from source creation without overlap.
    /// Each interval must span at least one nanosecond. Endpoints are rounded
    /// down on the PIT's nanosecond timeline, retaining fractions between samples.
    /// Skipped time still advances the counter and consumes control changes.
    pub fn sample(&mut self, interval: Range<Timestamp>) -> f32 {
        self.advance_to(interval.start);
        let start = self.signal.state.at;
        let integral = self.advance_to(interval.end);
        let duration = self
            .clock
            .duration_since(self.signal.state.at, start)
            .as_nanos();
        (integral as f64 / (duration * u128::from(INPUT_HZ)) as f64) as f32
    }

    fn advance_to(&mut self, end: Timestamp) -> u128 {
        let mut changes = self.changes.borrow_mut();
        let mut integral = 0;
        while changes.front().is_some_and(|change| change.state.at <= end) {
            let change = changes.pop_front().unwrap();
            integral += self.signal.advance_to(&self.clock, change.state.at);
            self.signal = change;
        }
        integral + self.signal.advance_to(&self.clock, end)
    }
}
