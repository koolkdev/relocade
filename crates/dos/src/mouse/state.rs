//! Sampled buttons, independently consumed counters and logical coordinates.

use super::motion::{Acceleration, Axis};
use relocade_machine::{FarPointer, Instant, MemoryView};

#[derive(Clone, Copy, Default)]
pub(super) struct Point {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Copy, Default)]
pub(super) struct Counter {
    pub count: u16,
    pub position: Point,
}

#[derive(Clone, Copy)]
pub(super) struct Callback {
    pub mask: u16,
    pub target: FarPointer,
}

impl Default for Callback {
    fn default() -> Self {
        Self {
            mask: 0,
            target: FarPointer {
                segment: 0,
                offset: 0,
            },
        }
    }
}

pub(super) struct Event {
    pub mask: u16,
    pub buttons: u16,
    pub position: Point,
    pub motion: [i16; 2],
}

pub(super) struct State {
    pub x: Axis,
    pub y: Axis,
    pub acceleration: Acceleration,
    pub buttons: u16,
    pub pressed: [Counter; 3],
    pub released: [Counter; 3],
    pub motion: [i16; 2],
    pub callback: Callback,
}

impl Default for State {
    fn default() -> Self {
        Self {
            x: Axis::new(639, 8),
            y: Axis::new(199, 16),
            acceleration: Acceleration::default(),
            buttons: 0,
            pressed: [Counter::default(); 3],
            released: [Counter::default(); 3],
            motion: [0; 2],
            callback: Callback::default(),
        }
    }
}

impl State {
    pub fn position(&self, memory: &MemoryView<'_>) -> Point {
        let mode = relocade_bios::video::mode(memory);
        let (x_grid, y_grid) = match mode.mode & 0x7f {
            0 | 1 => (16, 8),
            2 | 3 | 7 => (8, 8),
            0x13 => (2, 1),
            _ => (1, 1),
        };
        Point {
            x: (self.x.position.div_euclid(x_grid) * x_grid) as u16,
            y: (self.y.position.div_euclid(y_grid) * y_grid) as u16,
        }
    }

    pub fn packet(
        &mut self,
        memory: &MemoryView<'_>,
        now: Instant,
        buttons: u16,
        x: i16,
        y: i16,
    ) -> Event {
        let doubled = self.acceleration.sample(now, x, y);
        self.x.advance(x, doubled);
        self.y.advance(y, doubled);
        self.motion[0] = self.motion[0].wrapping_add(x);
        self.motion[1] = self.motion[1].wrapping_add(y);
        let position = self.position(memory);
        let mut mask = u16::from(x != 0 || y != 0);
        for button in 0..3 {
            let bit = 1 << button;
            if (self.buttons ^ buttons) & bit != 0 {
                let pressed = buttons & bit != 0;
                let counter = if pressed {
                    &mut self.pressed[button]
                } else {
                    &mut self.released[button]
                };
                counter.count = counter.count.wrapping_add(1);
                counter.position = position;
                mask |= 1 << (1 + 2 * button + usize::from(!pressed));
            }
        }
        self.buttons = buttons;
        Event {
            mask,
            buttons,
            position,
            motion: self.motion,
        }
    }

    pub fn take_counter(&mut self, button: u16, pressed: bool) -> Counter {
        let counters = if pressed {
            &mut self.pressed
        } else {
            &mut self.released
        };
        let Some(counter) = counters.get_mut(usize::from(button)) else {
            return Counter::default();
        };
        let result = *counter;
        counter.count = 0;
        result
    }
}
