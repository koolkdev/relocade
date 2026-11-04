//! Keyboard scanning, command replies and typematic timing before the 8042.

use crate::scancodes;
use relocade_frontend::keyboard::{ElementState, KeyCode, KeyboardEvent, PhysicalKey};
use std::{
    collections::{BTreeSet, VecDeque},
    time::Duration,
};

#[derive(Clone, Copy)]
enum Parameter {
    Leds,
    Typematic,
    ScanSet,
}

pub(super) struct Keyboard {
    held: BTreeSet<KeyCode>,
    scans: VecDeque<u8>,
    replies: VecDeque<u8>,
    overflow: bool,
    parameter: Option<Parameter>,
    scanning: bool,
    set: u8,
    leds: u8,
    typematic: u8,
    repeating: Option<KeyCode>,
    pub deadline: Option<Duration>,
    last: u8,
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            held: BTreeSet::new(),
            scans: VecDeque::new(),
            replies: VecDeque::new(),
            overflow: false,
            parameter: None,
            scanning: true,
            set: 2,
            leds: 0,
            typematic: 0x2b,
            repeating: None,
            deadline: None,
            last: 0xfe,
        }
    }
}

impl Keyboard {
    pub fn event(&mut self, event: KeyboardEvent, now: Duration) {
        use KeyCode::*;
        match event {
            KeyboardEvent::Key {
                physical_key: PhysicalKey::Code(key),
                state,
                ..
            } => {
                self.key(key, state == ElementState::Pressed, now);
            }
            KeyboardEvent::FocusLost => {
                for key in self.held.clone() {
                    self.key(key, false, now);
                }
            }
            KeyboardEvent::Modifiers(modifiers) => {
                for (active, left, right) in [
                    (modifiers.shift_key(), ShiftLeft, ShiftRight),
                    (modifiers.control_key(), ControlLeft, ControlRight),
                    (modifiers.alt_key(), AltLeft, AltRight),
                    (modifiers.super_key(), SuperLeft, SuperRight),
                ] {
                    if !active {
                        self.key(left, false, now);
                        self.key(right, false, now);
                    } else if !self.held.contains(&left) && !self.held.contains(&right) {
                        self.key(left, true, now);
                    }
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, key: KeyCode, pressed: bool, now: Duration) {
        if !scancodes::supported(key) {
            return;
        }
        let changed = if pressed {
            self.held.insert(key)
        } else {
            self.held.remove(&key)
        };
        if !changed {
            return;
        } // The guest's typematic settings own repetition.
        if self.scanning && self.parameter.is_none() {
            self.emit(key, pressed);
            if pressed && scancodes::repeatable(key) {
                self.repeating = Some(key);
                self.deadline = Some(now.saturating_add(self.delay()));
            }
        }
        if !pressed && self.repeating == Some(key) {
            self.stop_repeat();
        }
    }

    fn emit(&mut self, key: KeyCode, pressed: bool) {
        let bytes = scancodes::sequence(key, pressed, self.set, &self.held);
        // Admit whole sequences so an overflow cannot leave a dangling prefix.
        if self.scans.len() + bytes.len() <= 16 {
            self.scans.extend(bytes);
        } else {
            self.overflow = true;
        }
    }

    fn delay(&self) -> Duration {
        Duration::from_millis(250 * (u64::from((self.typematic >> 5) & 3) + 1))
    }

    pub fn repeat(&mut self, now: Duration, inhibited: bool) {
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            if let Some(key) = self.repeating {
                if !inhibited {
                    self.emit(key, true);
                }
                // Rate = 240 / ((8 + A) * 2^B) Hz, per the keyboard command.
                let clocks = u64::from(8 + (self.typematic & 7)) << ((self.typematic >> 3) & 3);
                let period = Duration::from_nanos((clocks * 1_000_000_000).div_ceil(240));
                self.deadline = Some(now.saturating_add(period));
            }
        }
    }

    fn stop_repeat(&mut self) {
        self.repeating = None;
        self.deadline = None;
    }

    fn clear_scans(&mut self) {
        self.scans.clear();
        self.overflow = false;
        self.stop_repeat();
    }

    fn defaults(&mut self, scanning: bool) {
        self.clear_scans();
        self.parameter = None;
        self.scanning = scanning;
        self.set = 2;
        self.typematic = 0x2b;
        self.leds = 0;
    }

    pub fn command(&mut self, value: u8) {
        // A new command aborts an unfinished option-byte exchange. RESEND
        // repeats the last reply without changing the expected option.
        if value == 0xfe {
            self.replies.push_back(self.last);
            return;
        }
        if value >= 0xed {
            self.parameter = None;
        }
        if let Some(parameter) = self.parameter.take() {
            let valid = match parameter {
                Parameter::Leds if value < 8 => {
                    self.leds = value;
                    true
                }
                Parameter::Typematic if value < 128 => {
                    self.typematic = value;
                    true
                }
                Parameter::ScanSet if value <= 2 => {
                    if value == 0 {
                        self.replies.extend([0xfa, self.set]);
                        return;
                    }
                    self.set = value;
                    true
                }
                _ => false,
            };
            if !valid {
                self.parameter = Some(parameter);
            }
            self.replies.push_back(if valid { 0xfa } else { 0xfe });
            return;
        }
        match value {
            0xed | 0xf0 | 0xf3 => {
                self.stop_repeat();
                self.parameter = Some(match value {
                    0xed => Parameter::Leds,
                    0xf0 => Parameter::ScanSet,
                    _ => Parameter::Typematic,
                });
                if value == 0xf0 {
                    self.clear_scans();
                }
                self.replies.push_back(0xfa);
            }
            0xee => self.replies.push_back(0xee),
            0xf2 => {
                self.stop_repeat();
                self.replies.extend([0xfa, 0xab, 0x83]);
            }
            0xf4 => {
                self.clear_scans();
                self.scanning = true;
                self.replies.push_back(0xfa);
            }
            0xf5 | 0xf6 | 0xff => {
                self.defaults(value != 0xf5);
                self.replies.push_back(0xfa);
                if value == 0xff {
                    self.replies.push_back(0xaa);
                }
            }
            _ => self.replies.push_back(0xfe),
        }
    }

    pub fn next_byte(&mut self) -> Option<u8> {
        let byte = self
            .replies
            .pop_front()
            .or_else(|| self.scans.pop_front())
            .or_else(|| {
                std::mem::take(&mut self.overflow).then_some(if self.set == 1 { 0xff } else { 0 })
            })?;
        if byte != 0xfe {
            self.last = byte;
        }
        Some(byte)
    }
}
