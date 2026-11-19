//! Keyboard commands, physical scanning, bounded buffering and typematic timing.

use crate::{
    protocol::{ACK, RESEND, SELF_TEST_PASSED},
    scancodes,
};
use relocade_frontend::keyboard::{ElementState, KeyCode, KeyboardEvent, PhysicalKey};
use relocade_machine::{Clock, Timestamp};
use std::{
    collections::{BTreeSet, VecDeque},
    time::Duration,
};

const SET_LEDS: u8 = 0xed;
const ECHO: u8 = 0xee;
const SCAN_SET: u8 = 0xf0;
const IDENTIFY: u8 = 0xf2;
const SET_TYPEMATIC: u8 = 0xf3;
const ENABLE_SCANNING: u8 = 0xf4;
const DEFAULT_DISABLE: u8 = 0xf5;
const DEFAULTS: u8 = 0xf6;
const RESET: u8 = 0xff;

pub(super) struct Keyboard {
    clock: Clock,
    replies: VecDeque<u8>,
    after_reply: AfterReply,
    last_sent: u8,
    held: BTreeSet<KeyCode>,
    scans: VecDeque<u8>,
    overflow: bool,
    // Physical keys at the last scan before a command suspended scanning.
    paused: Option<BTreeSet<KeyCode>>,
    scanning: bool,
    set: u8,
    leds: u8,
    typematic: u8,
    repeat: Option<Repeat>,
}

#[derive(Clone, Copy)]
struct Repeat {
    key: KeyCode,
    deadline: Timestamp,
}

#[derive(Clone, Copy)]
enum AfterReply {
    Resume,
    AwaitOption(u8),
    Reset,
}

impl Keyboard {
    pub(super) fn new(clock: Clock) -> Self {
        Self {
            clock,
            replies: VecDeque::new(),
            after_reply: AfterReply::Resume,
            last_sent: RESEND,
            held: BTreeSet::new(),
            scans: VecDeque::new(),
            overflow: false,
            paused: None,
            scanning: true,
            set: 2,
            leds: 0,
            typematic: 0x2b,
            repeat: None,
        }
    }

    pub(super) fn command(&mut self, command: u8) {
        if self.replies.is_empty() {
            if let AfterReply::AwaitOption(setting) = self.after_reply {
                if command == RESEND {
                    self.replies.push_back(self.last_sent);
                    return;
                }
                if command < SET_LEDS {
                    self.configure(setting, command);
                    return;
                }
            }
        }

        // A command supersedes any byte still waiting for the controller.
        self.replies.clear();
        self.after_reply = AfterReply::Resume;
        self.resume();
        match command {
            SET_LEDS | SCAN_SET | SET_TYPEMATIC => {
                self.pause();
                if command == SCAN_SET {
                    self.clear_scans();
                }
                self.replies.push_back(ACK);
                self.after_reply = AfterReply::AwaitOption(command);
            }
            ECHO => self.replies.push_back(ECHO),
            IDENTIFY => {
                self.pause();
                self.replies.extend([ACK, 0xab, 0x83]);
            }
            ENABLE_SCANNING => {
                self.clear_scans();
                self.scanning = true;
                self.replies.push_back(ACK);
            }
            DEFAULT_DISABLE | DEFAULTS => {
                self.defaults(command != DEFAULT_DISABLE);
                self.replies.push_back(ACK);
            }
            RESET => {
                self.pause();
                self.replies.push_back(ACK);
                self.after_reply = AfterReply::Reset;
            }
            RESEND => self.replies.push_back(self.last_sent),
            _ => self.replies.push_back(RESEND),
        }
    }

    fn configure(&mut self, command: u8, value: u8) {
        match command {
            SET_LEDS if value < 8 => self.leds = value,
            SET_TYPEMATIC if value < 128 => self.typematic = value,
            SCAN_SET if value == 0 => {
                self.after_reply = AfterReply::Resume;
                self.replies.extend([ACK, self.set]);
                return;
            }
            SCAN_SET if value <= 2 => self.set = value,
            _ => {
                self.replies.push_back(RESEND);
                return;
            }
        }
        self.after_reply = AfterReply::Resume;
        self.replies.push_back(ACK);
    }

    pub(super) fn byte(&self) -> Option<u8> {
        if let Some(byte) = self.replies.front() {
            return Some(*byte);
        }
        if self.paused.is_some() {
            return None;
        }
        self.peek_byte()
    }

    /// Commits the byte just accepted by the controller. An unwritten reset ACK
    /// can still be replaced before the keyboard applies its defaults.
    pub(super) fn sent(&mut self) {
        let byte = self.byte().expect("keyboard had a byte for the controller");
        if byte != RESEND {
            self.last_sent = byte;
        }
        if self.replies.pop_front().is_none() {
            self.consume_byte();
        } else if self.replies.is_empty() {
            match self.after_reply {
                AfterReply::Resume => self.resume(),
                AfterReply::AwaitOption(_) => {}
                AfterReply::Reset => {
                    self.defaults(true);
                    self.replies.push_back(SELF_TEST_PASSED);
                    self.after_reply = AfterReply::Resume;
                }
            }
        }
    }

    pub(super) fn deadline(&self) -> Option<Timestamp> {
        self.repeat.map(|repeat| repeat.deadline)
    }

    pub(super) fn event(&mut self, event: KeyboardEvent) {
        use KeyCode::*;
        let now = self.clock.now();
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

    fn key(&mut self, key: KeyCode, pressed: bool, now: Timestamp) {
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
        if !pressed && self.repeat.is_some_and(|repeat| repeat.key == key) {
            self.repeat = None;
        }
    }

    fn scan(&mut self, key: KeyCode, pressed: bool, now: Timestamp) {
        self.emit(key, pressed);
        if pressed && scancodes::repeatable(key) {
            self.repeat = Some(Repeat {
                key,
                deadline: self.clock.deadline_after(now, self.delay()),
            });
        }
    }

    fn pause(&mut self) {
        if self.paused.is_none() {
            self.paused = Some(self.held.clone());
        }
        self.repeat = None;
    }

    fn resume(&mut self) {
        if let Some(before) = self.paused.take() {
            if self.scanning {
                let now = self.clock.now();
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

    pub(super) fn repeat(&mut self, inhibited: bool) {
        let now = self.clock.now();
        if let Some(repeat) = self.repeat.filter(|repeat| now >= repeat.deadline) {
            if !inhibited {
                self.emit(repeat.key, true);
            }
            // Rate = 240 / ((8 + A) * 2^B) Hz, per the keyboard command.
            let clocks = u64::from(8 + (self.typematic & 7)) << ((self.typematic >> 3) & 3);
            let period = Duration::from_nanos((clocks * 1_000_000_000).div_ceil(240));
            self.repeat = Some(Repeat {
                deadline: self.clock.deadline_after(now, period),
                ..repeat
            });
        }
    }

    fn clear_scans(&mut self) {
        self.scans.clear();
        self.overflow = false;
        self.repeat = None;
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
