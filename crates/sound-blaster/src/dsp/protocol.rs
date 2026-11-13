use relocade_machine::{Clock, Timestamp};
use std::{collections::VecDeque, time::Duration};

const INITIALIZATION_TIME: Duration = Duration::from_micros(100);
const GET_VERSION: u8 = 0xe1;
const SET_TIME_CONSTANT: u8 = 0x40;
const SINGLE_CYCLE_OUTPUT: u8 = 0x14;
const SPEAKER_ON: u8 = 0xd1;
const SPEAKER_OFF: u8 = 0xd3;
const SPEAKER_STATUS: u8 = 0xd8;
const DIRECT_OUTPUT: u8 = 0x10;

enum Argument {
    DirectOutput,
    TimeConstant,
    LengthLow,
    LengthHigh(u8),
}

pub(super) enum Command {
    DirectOutput(u8),
    TimeConstant(u8),
    Playback(u16),
    Speaker(bool),
    SpeakerStatus,
}

#[derive(Default)]
pub(super) struct Protocol {
    reset_high: bool,
    ready_at: Option<Timestamp>,
    replies: VecDeque<u8>,
    argument: Option<Argument>,
}

impl Protocol {
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

    pub fn respond(&mut self, bytes: impl IntoIterator<Item = u8>) {
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

    pub fn write(&mut self, clock: &Clock, value: u8) -> Option<Command> {
        if !self.ready(clock) {
            return None;
        }
        if let Some(argument) = self.argument.take() {
            return match argument {
                Argument::DirectOutput => Some(Command::DirectOutput(value)),
                Argument::TimeConstant => Some(Command::TimeConstant(value)),
                Argument::LengthLow => {
                    self.argument = Some(Argument::LengthHigh(value));
                    None
                }
                Argument::LengthHigh(low) => {
                    Some(Command::Playback(u16::from_le_bytes([low, value])))
                }
            };
        }
        match value {
            DIRECT_OUTPUT => self.argument = Some(Argument::DirectOutput),
            GET_VERSION => self.respond([2, 1]),
            SET_TIME_CONSTANT => self.argument = Some(Argument::TimeConstant),
            SINGLE_CYCLE_OUTPUT => self.argument = Some(Argument::LengthLow),
            SPEAKER_ON => return Some(Command::Speaker(true)),
            SPEAKER_OFF => return Some(Command::Speaker(false)),
            SPEAKER_STATUS => return Some(Command::SpeakerStatus),
            _ => {}
        }
        None
    }
}
