//! Standard three-button PS/2 commands and sample scheduling.

use super::{motion::Motion, transmission::Transmission};
use relocade_frontend::mouse::MouseEvent;
use std::time::Duration;

const RESEND: u8 = 0xfe;
const RESET: u8 = 0xff;
const SET_DEFAULTS: u8 = 0xf6;
const DISABLE: u8 = 0xf5;
const ENABLE: u8 = 0xf4;
const SET_SAMPLE_RATE: u8 = 0xf3;
const IDENTIFY: u8 = 0xf2;
const SET_REMOTE_MODE: u8 = 0xf0;
const SET_WRAP_MODE: u8 = 0xee;
const RESET_WRAP_MODE: u8 = 0xec;
const READ_DATA: u8 = 0xeb;
const SET_STREAM_MODE: u8 = 0xea;
const STATUS: u8 = 0xe9;
const SET_RESOLUTION: u8 = 0xe8;
const SET_SCALING: u8 = 0xe7;
const RESET_SCALING: u8 = 0xe6;

#[derive(Clone, Copy)]
enum Parameter {
    Resolution,
    SampleRate,
}

pub(super) struct Protocol {
    motion: Motion,
    resolution: u8,
    sample_rate: u8,
    scaling: bool,
    enabled: bool,
    remote: bool,
    wrapped: bool,
    parameter: Option<Parameter>,
    invalid_inputs: u8,
    sampled_at: Duration,
    transmission: Transmission,
}

impl Protocol {
    pub fn new(now: Duration) -> Self {
        Self {
            motion: Motion::default(),
            resolution: 2,
            sample_rate: 100,
            scaling: false,
            enabled: false,
            remote: false,
            wrapped: false,
            parameter: None,
            invalid_inputs: 0,
            sampled_at: now,
            transmission: Transmission::new(),
        }
    }

    fn streaming(&self) -> bool {
        self.enabled && !self.remote && !self.wrapped && self.parameter.is_none()
    }

    pub fn event(&mut self, event: MouseEvent) {
        self.motion.event(event, self.resolution, self.streaming());
    }

    pub fn byte(&self) -> Option<u8> {
        self.transmission.byte(&self.motion)
    }

    pub fn sent(&mut self, now: Duration) {
        // Start the interval when a movement packet's header reaches the controller.
        if self.transmission.sent(&mut self.motion) {
            self.sampled_at = now;
        }
    }

    fn invalid(&mut self) {
        self.invalid_inputs = self.invalid_inputs.saturating_add(1);
        self.transmission.invalid(if self.invalid_inputs == 1 {
            RESEND
        } else {
            0xfc
        });
    }

    pub fn command(&mut self, byte: u8, now: Duration) {
        // BAT and device ID complete reset before another command is accepted.
        if self.transmission.resetting() {
            return;
        }
        if self.wrapped && byte != RESET && byte != RESET_WRAP_MODE {
            self.transmission.reply(false, &[byte]);
            return;
        }
        if byte == RESEND {
            self.transmission.resend();
            return;
        }
        if let Some(parameter) = self.parameter {
            if byte < RESET_SCALING {
                let valid = match parameter {
                    Parameter::Resolution if byte <= 3 => {
                        self.resolution = byte;
                        true
                    }
                    Parameter::SampleRate if [10, 20, 40, 60, 80, 100, 200].contains(&byte) => {
                        self.sample_rate = byte;
                        true
                    }
                    _ => false,
                };
                if valid {
                    self.parameter = None;
                    self.invalid_inputs = 0;
                    self.motion.clear();
                    self.sampled_at = now;
                    self.transmission.reply(true, &[]);
                } else {
                    self.invalid();
                }
                return;
            }
        }
        let interrupted_parameter = self.parameter.take();
        // Read Data observes the current counters before the command clears them.
        if byte == READ_DATA {
            self.invalid_inputs = 0;
            self.transmission.read_data(&mut self.motion);
            return;
        }
        self.motion.clear();
        match byte {
            RESET | SET_DEFAULTS => {
                self.resolution = 2;
                self.sample_rate = 100;
                self.scaling = false;
                self.enabled = false;
                self.remote = false;
                self.wrapped = false;
                self.motion.reset();
                self.sampled_at = now;
                if byte == RESET {
                    self.transmission.reset();
                } else {
                    self.transmission.reply(true, &[]);
                }
            }
            DISABLE => {
                self.enabled = false;
                self.transmission.reply(true, &[]);
            }
            ENABLE => {
                self.enabled = true;
                self.sampled_at = now;
                self.transmission.reply(true, &[]);
            }
            SET_SAMPLE_RATE | SET_RESOLUTION => {
                self.parameter = Some(if byte == SET_SAMPLE_RATE {
                    Parameter::SampleRate
                } else {
                    Parameter::Resolution
                });
                self.transmission.reply(true, &[]);
            }
            IDENTIFY => self.transmission.reply(true, &[0]),
            SET_REMOTE_MODE => {
                self.remote = true;
                self.transmission.reply(true, &[]);
            }
            SET_WRAP_MODE => {
                self.wrapped = true;
                self.enabled = false;
                self.transmission.reply(true, &[]);
            }
            RESET_WRAP_MODE => {
                self.wrapped = false;
                self.transmission.reply(true, &[]);
            }
            SET_STREAM_MODE => {
                self.remote = false;
                self.sampled_at = now;
                self.transmission.reply(true, &[]);
            }
            STATUS => {
                let buttons = self.motion.buttons;
                let status = (u8::from(self.remote) << 6)
                    | (u8::from(self.enabled) << 5)
                    | (u8::from(self.scaling) << 4)
                    | ((buttons & 1) << 2)
                    | ((buttons & 2) >> 1)
                    | ((buttons & 4) >> 1);
                self.transmission
                    .reply(true, &[status, self.resolution, self.sample_rate]);
            }
            RESET_SCALING | SET_SCALING => {
                self.scaling = byte == SET_SCALING;
                self.transmission.reply(true, &[]);
            }
            _ => {
                self.parameter = interrupted_parameter;
                self.invalid();
                return;
            }
        }
        self.invalid_inputs = 0;
    }

    pub fn deadline(&self) -> Option<Duration> {
        (self.streaming() && self.transmission.idle() && self.motion.pending()).then(|| {
            self.sampled_at.saturating_add(Duration::from_nanos(
                1_000_000_000_u64.div_ceil(u64::from(self.sample_rate)),
            ))
        })
    }

    pub fn sample(&mut self, now: Duration) {
        if self.deadline().is_some_and(|deadline| now >= deadline) {
            self.transmission.stream(self.scaling);
        }
    }
}
