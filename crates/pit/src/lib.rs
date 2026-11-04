//! Binary 8254 counters and the PC/AT input clock.

mod timer;
pub use timer::Pit;

mod clock;
pub use clock::{clocks_at, time_at, INPUT_HZ};

/// PC/AT I/O port for the channel-0 count register.
pub const CHANNEL_0_PORT: u16 = 0x40;
/// PC/AT I/O port for the channel-2 count register.
pub const CHANNEL_2_PORT: u16 = 0x42;
/// PC/AT I/O port for PIT control and read-back commands.
pub const CONTROL_PORT: u16 = 0x43;
/// Registers decoded by the PIT device.
pub const PORT_RANGE: std::ops::RangeInclusive<u16> = CHANNEL_0_PORT..=CONTROL_PORT;

/// One counter, advanced in whole input clocks. See the crate README for scope.
///
/// Register writes take effect at the current time; initial loads and gate
/// triggers load on the next clock. A running counter reloads at its next period
/// (mode 2) or half-period (mode 3), preserving the current phase until then.
#[derive(Clone, Debug, Default)]
pub struct Counter {
    control: u8,
    gate: bool,
    reload: u32,
    active: u32,
    phase: u32,
    loading: bool,
    null_count: bool,
    write_low: Option<u8>,
    read_high: bool,
    latched_count: Option<u16>,
    latched_status: Option<u8>,
}

impl Counter {
    /// Program the low six bits of a control word. Access mode zero latches the
    /// count without changing the existing configuration.
    pub fn control(&mut self, value: u8) {
        if value & 0x30 == 0 {
            self.latch(true, false);
            return;
        }
        *self = Self {
            control: value & 0x3f,
            gate: self.gate,
            null_count: true,
            ..Self::default()
        };
    }

    /// Write a count byte in the programmed access order.
    pub fn write(&mut self, value: u8) {
        let count = match self.control & 0x30 {
            0x10 => u16::from(value),
            0x20 => u16::from(value) << 8,
            0x30 => match self.write_low.take() {
                Some(low) => u16::from_le_bytes([low, value]),
                None => {
                    self.write_low = Some(value);
                    return;
                }
            },
            _ => return,
        };
        self.reload = if count == 0 { 65536 } else { count.into() };
        self.null_count = true;
        if self.active == 0 {
            self.loading = true;
        }
    }

    /// Read pending status first, then count bytes in the programmed order.
    pub fn read(&mut self) -> u8 {
        if let Some(status) = self.latched_status.take() {
            return status;
        }
        let count = self.latched_count.unwrap_or_else(|| self.count());
        let high = self.control & 0x30 == 0x20 || (self.control & 0x30 == 0x30 && self.read_high);
        self.read_high = self.control & 0x30 == 0x30 && !self.read_high;
        if !self.read_high {
            self.latched_count = None;
        }
        if high {
            (count >> 8) as u8
        } else {
            count as u8
        }
    }

    /// Preserve each requested value until it has been read completely.
    pub fn latch(&mut self, count: bool, status: bool) {
        if count && self.latched_count.is_none() {
            self.latched_count = Some(self.count());
        }
        if status && self.latched_status.is_none() {
            self.latched_status = Some(
                self.control | (u8::from(self.null_count) << 6) | (u8::from(self.output()) << 7),
            );
        }
    }

    /// A low gate forces OUT high; a rising gate reloads on the next clock.
    pub fn set_gate(&mut self, high: bool) {
        if high && !self.gate && self.reload != 0 {
            self.loading = true;
        }
        self.gate = high;
    }

    pub fn output(&self) -> bool {
        self.mode().is_some()
            && (!self.gate || self.loading || self.active == 0 || self.phase < self.high_clocks())
    }

    /// Advance input clocks and return how many elapsed clock intervals had OUT
    /// high. This also integrates many periods without iterating over edges.
    pub fn advance(&mut self, mut clocks: u64) -> u64 {
        let mut high = 0;
        if self.loading && clocks != 0 {
            high += u64::from(self.output());
            self.load();
            self.phase = 0;
            self.loading = false;
            clocks -= 1;
        }
        if self.mode().is_none() || self.active == 0 || !self.gate {
            return high + clocks * u64::from(self.output());
        }
        if self.null_count {
            let boundary = if self.mode() == Some(3) && self.phase < self.high_clocks() {
                self.high_clocks()
            } else {
                self.active
            };
            let until_reload = u64::from(boundary - self.phase);
            if clocks >= until_reload {
                let next_high = boundary == self.active;
                high += self.advance_periodic(until_reload);
                self.load();
                self.phase = if next_high { 0 } else { self.high_clocks() };
                clocks -= until_reload;
            }
        }
        if self.active == 0 {
            high + clocks * u64::from(self.output())
        } else {
            high + self.advance_periodic(clocks)
        }
    }

    /// Clocks to the next load or output transition in a running counter.
    fn next_transition(&self) -> Option<u64> {
        if self.mode().is_none() || !self.gate {
            None
        } else if self.loading {
            Some(1)
        } else if self.active == 0 {
            None
        } else {
            let boundary = if self.phase < self.high_clocks() {
                self.high_clocks()
            } else {
                self.active
            };
            Some(u64::from(boundary - self.phase))
        }
    }

    fn next_rising_edge(&self) -> Option<u64> {
        let mut counter = self.clone();
        let mut clocks = 0;
        // At most an initial load, a falling edge, then a rising edge. Pending
        // reloads use advance's mode-specific boundary, even across a long pause.
        while let Some(until) = counter.next_transition() {
            let high = counter.output();
            counter.advance(until);
            clocks += until;
            if !high && counter.output() {
                return Some(clocks);
            }
        }
        None
    }

    fn mode(&self) -> Option<u8> {
        let mode = (self.control >> 1) & 3;
        (self.control & 1 == 0 && matches!(mode, 2 | 3)).then_some(mode)
    }

    fn load(&mut self) {
        // Count 1 is outside Intel's valid range for either supported mode.
        self.active = if self.reload >= 2 { self.reload } else { 0 };
        self.null_count = false;
    }

    fn high_clocks(&self) -> u32 {
        if self.mode() == Some(3) {
            self.active.div_ceil(2)
        } else {
            self.active.saturating_sub(1)
        }
    }

    fn count(&self) -> u16 {
        let count = if self.mode() == Some(2) {
            self.active - self.phase
        } else if self.active == 0 {
            0
        } else {
            let half_phase = if self.phase < self.high_clocks() {
                self.phase
            } else {
                self.phase - self.high_clocks()
            };
            (self.active & !1).saturating_sub(2 * half_phase)
        };
        count as u16
    }

    fn advance_periodic(&mut self, clocks: u64) -> u64 {
        let period = u128::from(self.active);
        let high = u128::from(self.high_clocks());
        let end = u128::from(self.phase) + u128::from(clocks);
        let integral =
            end / period * high + (end % period).min(high) - u128::from(self.phase).min(high);
        self.phase = (end % period) as u32;
        integral as u64
    }
}

#[cfg(test)]
mod tests;
