//! Relative counters and sampled button latches, independent of transport.

use relocade_frontend::mouse::{ElementState, MouseButton, MouseEvent};

#[derive(Default)]
pub(super) struct Motion {
    pub buttons: u8,
    reported: u8,
    pressed: u8,
    fraction: [f64; 2],
    x: Axis,
    y: Axis,
    moved: bool,
}

pub(super) struct Sample {
    buttons: u8,
    x: Axis,
    y: Axis,
}

#[derive(Clone, Copy, Default)]
struct Axis {
    count: i16,
    overflow: bool,
}

impl Axis {
    fn add(&mut self, delta: i16) {
        let total = self.count + delta;
        self.overflow |= !(-256..=255).contains(&total);
        self.count = total.clamp(-256, 255);
    }

    fn report(self, scaling: bool) -> (u8, bool, bool) {
        let mut count = self.count;
        if scaling {
            count = count.signum()
                * match count.abs() {
                    0 => 0,
                    1 | 2 => 1,
                    3 => 3,
                    4 => 6,
                    5 => 9,
                    n => n * 2,
                };
        }
        let overflow = self.overflow || !(-256..=255).contains(&count);
        count = count.clamp(-256, 255);
        (count as u8, count < 0, overflow)
    }
}

impl Motion {
    pub fn clear(&mut self) {
        self.x = Axis::default();
        self.y = Axis::default();
        self.moved = false;
        self.pressed = 0;
        self.fraction = [0.0; 2];
    }

    pub fn reset(&mut self) {
        self.clear();
        self.reported = 0;
    }

    pub fn report_sent(&mut self, sample: Sample) {
        self.reported = sample.buttons;
    }

    pub fn pending(&self) -> bool {
        self.moved || self.pressed != 0 || self.buttons != self.reported
    }

    pub fn event(&mut self, event: MouseEvent, resolution: u8, latch_presses: bool) {
        match event {
            MouseEvent::Motion { delta_x, delta_y }
                if delta_x.is_finite() && delta_y.is_finite() =>
            {
                // One host unit is one count at the default four counts/mm.
                // Bound only values already far beyond the nine-bit counters.
                let scale = f64::from(1 << resolution) / 4.0;
                let mut counts = [0; 2];
                for (axis, delta) in [delta_x, -delta_y].into_iter().enumerate() {
                    let total = (self.fraction[axis] + delta * scale).clamp(-512.0, 512.0);
                    let whole = total.trunc();
                    self.fraction[axis] = total - whole;
                    counts[axis] = whole as i16;
                }
                self.x.add(counts[0]);
                self.y.add(counts[1]);
                self.moved |= counts != [0, 0];
            }
            MouseEvent::Button { button, state } => {
                let bit = match button {
                    MouseButton::Left => 1,
                    MouseButton::Right => 2,
                    MouseButton::Middle => 4,
                    _ => return,
                };
                let buttons = if state == ElementState::Pressed {
                    self.buttons | bit
                } else {
                    self.buttons & !bit
                };
                if latch_presses {
                    self.pressed |= buttons & !self.buttons;
                }
                self.buttons = buttons;
            }
            MouseEvent::FocusLost => self.buttons = 0,
            _ => {}
        }
    }

    pub fn snapshot(&self, stream: bool) -> Sample {
        // IBM stream reports latch a press even if it is released within the
        // interval. Repeated clicks coalesce; there is no transition backlog.
        Sample {
            buttons: self.buttons | if stream { self.pressed } else { 0 },
            x: self.x,
            y: self.y,
        }
    }

    pub fn sample(&mut self, stream: bool) -> Sample {
        let sample = self.snapshot(stream);
        self.pressed = 0;
        self.moved = false;
        self.x = Axis::default();
        self.y = Axis::default();
        sample
    }

    pub fn read_data(&mut self, interrupted: Option<Sample>) {
        self.pressed = 0;
        if let Some(mut sample) = interrupted {
            sample.x.add(self.x.count);
            sample.y.add(self.y.count);
            sample.x.overflow |= self.x.overflow;
            sample.y.overflow |= self.y.overflow;
            self.x = sample.x;
            self.y = sample.y;
        }
    }
}

impl Sample {
    pub fn packet(&self, scaling: bool) -> [u8; 3] {
        let (x, x_sign, x_overflow) = self.x.report(scaling);
        let (y, y_sign, y_overflow) = self.y.report(scaling);
        [
            8 | self.buttons
                | (u8::from(x_sign) << 4)
                | (u8::from(y_sign) << 5)
                | (u8::from(x_overflow) << 6)
                | (u8::from(y_overflow) << 7),
            x,
            y,
        ]
    }
}
