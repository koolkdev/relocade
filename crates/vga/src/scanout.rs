use crate::timing::Timing;
use std::time::Duration;

const DISPLAY_DISABLED: u8 = 1 << 0;
const VERTICAL_RETRACE: u8 = 1 << 3;

/// Raster position at `updated`. Register writes advance using the old timing
/// before reconfiguring, so programming cannot retime elapsed counters.
pub(super) struct Scanout {
    updated: Duration,
    character: u64,
    /// Fractional character position in the units maintained by `Timing`.
    character_remainder: u64,
}

impl Scanout {
    pub fn new(now: Duration) -> Self {
        Self {
            updated: now,
            character: 0,
            character_remainder: 0,
        }
    }

    pub fn advance(&mut self, now: Duration, timing: Timing) {
        let elapsed = now.saturating_sub(self.updated);
        self.updated = now;
        if !timing.clock_running() {
            return;
        }
        let characters = timing.elapsed_characters(elapsed, &mut self.character_remainder);
        self.character = ((u128::from(self.character) + characters)
            % u128::from(timing.frame_characters())) as u64;
    }

    pub fn reconfigure(&mut self, old: Timing, new: Timing) {
        if old.reset_active || new.reset_active {
            self.character = 0;
            self.character_remainder = 0;
        } else {
            let physical_line = self.character / old.horizontal_total;
            let vertical_count = physical_line / old.scanlines_per_vertical_count;
            let subline = physical_line % old.scanlines_per_vertical_count;
            let line = vertical_count * new.scanlines_per_vertical_count
                + subline * new.scanlines_per_vertical_count / old.scanlines_per_vertical_count;
            let horizontal = self.character % old.horizontal_total;
            self.character = (line * new.horizontal_total + horizontal % new.horizontal_total)
                % new.frame_characters();
            self.character_remainder = new.rescale_remainder(self.character_remainder, old);
        }
    }

    pub fn status(&self, timing: Timing) -> u8 {
        let horizontal = self.character % timing.horizontal_total;
        let vertical =
            self.character / timing.horizontal_total / timing.scanlines_per_vertical_count;
        // Input Status 1 reports inverted display enable, rather than the
        // separate blanking signal or the narrower horizontal sync pulse.
        let mut status = 0;
        if timing.reset_active
            || horizontal < timing.horizontal_display_skew
            || horizontal >= timing.horizontal_display_skew + timing.horizontal_display
            || vertical >= timing.vertical_display
        {
            status |= DISPLAY_DISABLED;
        }
        if timing.vertical_retrace_active(vertical) {
            status |= VERTICAL_RETRACE;
        }
        status
    }
}
