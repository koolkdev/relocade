//! Relative counters and button transitions, independent of command transport.

use relocade_frontend::mouse::{ElementState, MouseButton, MouseEvent};
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct Motion {
    pub buttons: u8,
    reported: u8,
    fraction: [f64; 2],
    samples: VecDeque<Sample>,
    in_flight: Option<Sample>,
}

#[derive(Clone, Copy, Default)]
struct Sample {
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
        self.samples.clear();
        self.in_flight = None;
        self.fraction = [0.0; 2];
    }

    pub fn reset(&mut self) {
        self.clear();
        self.reported = 0;
    }

    pub fn rescan_buttons(&mut self) {
        if self.buttons != self.reported {
            self.samples.push_back(Sample {
                buttons: self.buttons,
                ..Sample::default()
            });
        }
    }

    pub fn report_sent(&mut self) {
        self.reported = self
            .in_flight
            .take()
            .expect("completed motion report")
            .buttons;
    }

    pub fn pending(&self) -> bool {
        !self.samples.is_empty()
    }

    pub fn event(&mut self, event: MouseEvent, resolution: u8, preserve_transitions: bool) {
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
                if counts != [0, 0] {
                    if self.samples.is_empty() {
                        self.samples.push_back(Sample {
                            buttons: self.buttons,
                            ..Sample::default()
                        });
                    }
                    let sample = self.samples.back_mut().unwrap();
                    sample.x.add(counts[0]);
                    sample.y.add(counts[1]);
                }
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
                self.buttons(buttons, preserve_transitions);
            }
            MouseEvent::FocusLost => self.buttons(0, preserve_transitions),
            _ => {}
        }
    }

    fn buttons(&mut self, buttons: u8, preserve_transitions: bool) {
        if buttons == self.buttons {
            return;
        }
        self.buttons = buttons;
        if preserve_transitions {
            self.samples.push_back(Sample {
                buttons,
                ..Sample::default()
            });
        } else if let Some(sample) = self.samples.back_mut() {
            sample.buttons = buttons;
        }
    }

    pub fn packet(&mut self, scaling: bool) -> [u8; 3] {
        let sample = self.samples.pop_front().unwrap_or(Sample {
            buttons: self.buttons,
            ..Sample::default()
        });
        self.in_flight = Some(sample);
        sample.packet(scaling)
    }

    pub fn read_data(&mut self) -> [u8; 3] {
        let mut combined = Sample {
            buttons: self.buttons,
            ..Sample::default()
        };
        for sample in self
            .in_flight
            .take()
            .into_iter()
            .chain(self.samples.drain(..))
        {
            combined.x.add(sample.x.count);
            combined.y.add(sample.y.count);
            combined.x.overflow |= sample.x.overflow;
            combined.y.overflow |= sample.y.overflow;
        }
        self.in_flight = Some(combined);
        combined.packet(false)
    }
}

impl Sample {
    fn packet(self, scaling: bool) -> [u8; 3] {
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
