use relocade_machine::{Clock, Timestamp};
use std::{collections::VecDeque, time::Duration};

const INITIALIZATION_TIME: Duration = Duration::from_micros(100);
const GET_VERSION: u8 = 0xe1;

#[derive(Default)]
pub(super) struct Dsp {
    reset_high: bool,
    ready_at: Option<Timestamp>,
    replies: VecDeque<u8>,
}

impl Dsp {
    pub fn reset(&mut self, clock: &Clock, value: u8) {
        if value & 1 != 0 {
            *self = Self {
                reset_high: true,
                ..Self::default()
            };
        } else if self.reset_high {
            self.reset_high = false;
            self.ready_at = Some(clock.add(clock.now(), INITIALIZATION_TIME));
        }
    }

    fn ready(&mut self, clock: &Clock) -> bool {
        if self.reset_high {
            return false;
        }
        if let Some(at) = self.ready_at {
            if clock.now() < at {
                return false;
            }
            self.ready_at = None;
            self.respond([0xaa]);
        }
        true
    }

    fn respond(&mut self, bytes: impl IntoIterator<Item = u8>) {
        self.replies.clear();
        self.replies.extend(bytes);
    }

    pub fn read_data(&mut self, clock: &Clock) -> u8 {
        self.ready(clock);
        self.replies.pop_front().unwrap_or(0xff)
    }

    pub fn read_status(&mut self, clock: &Clock) -> u8 {
        self.ready(clock);
        if self.replies.is_empty() {
            0
        } else {
            0x80
        }
    }

    pub fn write_status(&mut self, clock: &Clock) -> u8 {
        if self.ready(clock) {
            0
        } else {
            0x80
        }
    }

    pub fn write(&mut self, clock: &Clock, value: u8) {
        if self.ready(clock) && value == GET_VERSION {
            self.respond([2, 1]);
        }
    }
}
