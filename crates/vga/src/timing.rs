//! Fixed raster timing for the initial 80-by-25 text mode.
//! Register programming is retained for readback; programmable timing is separate.

use std::time::Duration;

const DOTS_PER_SECOND: u128 = 25_175_000;
const NANOS_PER_SECOND: u128 = 1_000_000_000;
const DOTS_PER_LINE: u128 = 800;
const VISIBLE_DOTS: u128 = 640;
const LINES_PER_FRAME: u128 = 449;
const VISIBLE_LINES: u128 = 400;
const RETRACE_START_LINE: u128 = 412;
const RETRACE_END_LINE: u128 = 414;
const DOTS_PER_FRAME: u128 = DOTS_PER_LINE * LINES_PER_FRAME;
const FIRST_FRAME_DOTS: u128 = RETRACE_START_LINE * DOTS_PER_LINE;

/// Counts completed retrace boundaries on the shared session timebase.
/// Absolute oscillator arithmetic keeps timer rounding from accumulating drift.
pub(super) struct Timing {
    origin: Duration,
    pub completed_frames: u64,
}

impl Timing {
    pub fn new(origin: Duration) -> Self {
        Self {
            origin,
            completed_frames: 0,
        }
    }

    fn elapsed_dots(&self, now: Duration) -> u128 {
        now.saturating_sub(self.origin).as_nanos() * DOTS_PER_SECOND / NANOS_PER_SECOND
    }

    /// Account for elapsed frames; return whether a newer boundary was crossed.
    pub fn advance_to(&mut self, now: Duration) -> bool {
        let dots = self.elapsed_dots(now);
        if dots < FIRST_FRAME_DOTS {
            return false;
        }
        let frames = u64::try_from(1 + (dots - FIRST_FRAME_DOTS) / DOTS_PER_FRAME)
            .expect("VGA frame sequence fits in u64");
        if frames <= self.completed_frames {
            return false;
        }
        self.completed_frames = frames;
        true
    }

    fn frame_time(&self, frame: u64) -> Duration {
        let dots = FIRST_FRAME_DOTS + u128::from(frame - 1) * DOTS_PER_FRAME;
        let nanos = (dots * NANOS_PER_SECOND).div_ceil(DOTS_PER_SECOND);
        self.origin
            + Duration::new(
                (nanos / NANOS_PER_SECOND) as u64,
                (nanos % NANOS_PER_SECOND) as u32,
            )
    }

    pub fn completed_at(&self) -> Duration {
        self.frame_time(self.completed_frames)
    }

    pub fn next_frame_at(&self) -> Duration {
        self.frame_time(
            self.completed_frames
                .checked_add(1)
                .expect("VGA frame sequence exhausted"),
        )
    }

    pub fn blink_on(&self) -> bool {
        self.completed_frames % 32 < 16
    }

    pub fn status(&self, now: Duration) -> u8 {
        let position = self.elapsed_dots(now) % DOTS_PER_FRAME;
        let line = position / DOTS_PER_LINE;
        let dot = position % DOTS_PER_LINE;
        // Input Status 1: inverted display enable (bit 0), vertical retrace (bit 3).
        u8::from(line >= VISIBLE_LINES || dot >= VISIBLE_DOTS)
            | (u8::from((RETRACE_START_LINE..RETRACE_END_LINE).contains(&line)) << 3)
    }
}
