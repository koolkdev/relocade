//! Physical mickeys, fractional logical movement and speed conversion.

use relocade_machine::Instant;

pub(super) const DEFAULT_THRESHOLD: u16 = 64;

pub(super) struct Axis {
    pub position: i32,
    pub bounds: [i32; 2],
    pub ratio: u16,
    pub sensitivity: u16,
    remainder: i64,
}

impl Axis {
    pub fn new(limit: i32, ratio: u16) -> Self {
        Self {
            position: (limit + 1) / 2,
            bounds: [0, limit],
            ratio,
            sensitivity: 50,
            remainder: 0,
        }
    }

    pub fn set_position(&mut self, position: i32) {
        self.position = position.clamp(self.bounds[0], self.bounds[1]);
        self.remainder = 0;
    }

    pub fn set_bounds(&mut self, first: i16, last: i16) {
        self.bounds = [i32::from(first.min(last)), i32::from(first.max(last))];
        self.set_position(self.position);
    }

    pub fn set_ratio(&mut self, ratio: u16) {
        if ratio != 0 {
            self.ratio = ratio;
            self.remainder = 0;
        }
    }

    pub fn set_sensitivity(&mut self, sensitivity: u16) {
        self.sensitivity = sensitivity.clamp(1, 100);
        self.remainder = 0;
    }

    pub fn advance(&mut self, mickeys: i16, doubled: bool) {
        // Q70180's sensitivity scale, interpolated between its five-unit settings.
        // 50 is unity. Keep fractional movement until it contributes a logical pixel.
        const SCALE: [i64; 21] = [
            0, 1, 2, 4, 8, 12, 16, 20, 24, 28, 32, 40, 48, 56, 64, 72, 80, 88, 96, 104, 112,
        ];
        let setting = usize::from(self.sensitivity);
        let lower = setting / 5;
        let gain =
            SCALE[lower] * 5 + (SCALE[(lower + 1).min(20)] - SCALE[lower]) * (setting % 5) as i64;
        let denominator = i64::from(self.ratio) * 32 * 5;
        let movement = self.remainder + i64::from(mickeys) * 8 * gain * if doubled { 2 } else { 1 };
        let position = self.position + (movement / denominator) as i32;
        self.position = position.clamp(self.bounds[0], self.bounds[1]);
        self.remainder = if position == self.position {
            movement % denominator
        } else {
            0
        };
    }
}

pub(super) struct Acceleration {
    pub threshold: u16,
    previous: Option<Instant>,
}

impl Default for Acceleration {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_THRESHOLD,
            previous: None,
        }
    }
}

impl Acceleration {
    pub fn set_threshold(&mut self, threshold: u16) {
        self.threshold = if threshold == 0 {
            DEFAULT_THRESHOLD
        } else {
            threshold
        };
    }

    pub fn sample(&mut self, now: Instant, x: i16, y: i16) -> bool {
        let previous = self.previous.replace(now);
        let Some(elapsed) = previous.and_then(|previous| now.checked_duration_since(previous))
        else {
            return false;
        };
        if elapsed.is_zero() {
            return false;
        }
        // Maximum-axis speed avoids floating point and keeps the threshold independent
        // of the guest's polling frequency. The first sample has no measured speed.
        let distance = u128::from(x.unsigned_abs().max(y.unsigned_abs()));
        distance * 1_000_000_000 > u128::from(self.threshold) * elapsed.as_nanos()
    }
}
