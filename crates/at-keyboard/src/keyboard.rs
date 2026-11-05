//! Physical key state, bounded scan buffering and guest typematic timing.

mod service;
pub(super) use service::run;

use crate::scancodes;
use relocade_frontend::keyboard::{ElementState, KeyCode, KeyboardEvent, PhysicalKey};
use std::{
    collections::{BTreeSet, VecDeque},
    time::Duration,
};

pub(super) struct Keyboard {
    held: BTreeSet<KeyCode>,
    scans: VecDeque<u8>,
    overflow: bool,
    // Physical keys at the last scan before a command suspended scanning.
    paused: Option<BTreeSet<KeyCode>>,
    scanning: bool,
    set: u8,
    leds: u8,
    typematic: u8,
    repeating: Option<KeyCode>,
    deadline: Option<Duration>,
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            held: BTreeSet::new(),
            scans: VecDeque::new(),
            overflow: false,
            paused: None,
            scanning: true,
            set: 2,
            leds: 0,
            typematic: 0x2b,
            repeating: None,
            deadline: None,
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
        if self.scanning && self.paused.is_none() {
            self.scan(key, pressed, now);
        }
        if !pressed && self.repeating == Some(key) {
            self.stop_repeat();
        }
    }

    fn scan(&mut self, key: KeyCode, pressed: bool, now: Duration) {
        self.emit(key, pressed);
        if pressed && scancodes::repeatable(key) {
            self.repeating = Some(key);
            self.deadline = Some(now.saturating_add(self.delay()));
        }
    }

    fn pause(&mut self) {
        if self.paused.is_none() {
            self.paused = Some(self.held.clone());
        }
        self.stop_repeat();
    }

    fn resume(&mut self, now: Duration) {
        if let Some(before) = self.paused.take() {
            if self.scanning {
                // Rescan the physical matrix: a release during an option or ID
                // exchange must not leave a previously reported modifier stuck.
                let held = self.held.clone();
                for key in before.difference(&held) {
                    self.scan(*key, false, now);
                }
                for key in held.difference(&before) {
                    self.scan(*key, true, now);
                }
            }
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
        self.scanning = scanning;
        self.set = 2;
        self.typematic = 0x2b;
        self.leds = 0;
    }

    fn peek_byte(&self) -> Option<u8> {
        self.scans.front().copied().or_else(|| {
            self.overflow
                .then_some(if self.set == 1 { 0xff } else { 0 })
        })
    }

    fn consume_byte(&mut self) {
        if self.scans.pop_front().is_none() {
            self.overflow = false;
        }
    }
}
