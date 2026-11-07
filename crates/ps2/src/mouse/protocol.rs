//! Standard three-button PS/2 commands and sample scheduling.

use super::motion::Motion;
use relocade_frontend::mouse::MouseEvent;
use std::{collections::VecDeque, time::Duration};

const ACK: u8 = 0xfa;
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
    resetting: bool,
    parameter: Option<Parameter>,
    invalid_inputs: u8,
    sampled_at: Duration,
    outgoing: VecDeque<u8>,
    replaying: VecDeque<u8>,
    completing_packet: Option<Vec<u8>>,
    reporting: bool,
    last_packet: Vec<u8>,
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
            resetting: false,
            parameter: None,
            invalid_inputs: 0,
            sampled_at: now,
            outgoing: VecDeque::new(),
            replaying: VecDeque::new(),
            completing_packet: None,
            reporting: false,
            last_packet: vec![RESEND],
        }
    }

    fn streaming(&self) -> bool {
        self.enabled && !self.remote && !self.wrapped && self.parameter.is_none()
    }

    pub fn event(&mut self, event: MouseEvent) {
        self.motion.event(event, self.resolution, self.streaming());
    }

    pub fn byte(&self) -> Option<u8> {
        self.replaying
            .front()
            .or_else(|| self.outgoing.front())
            .copied()
    }

    pub fn sent(&mut self, now: Duration) {
        if self.replaying.pop_front().is_some() {
            return;
        }
        // Start the interval when the header reaches the controller, even if
        // backpressure delayed a report that was already assembled.
        if self.reporting && self.outgoing.len() == 3 {
            self.sampled_at = now;
        }
        self.outgoing.pop_front();
        if self.outgoing.is_empty() {
            if let Some(packet) = self.completing_packet.take() {
                self.last_packet = packet;
            }
            if std::mem::take(&mut self.reporting) {
                self.motion.report_sent();
            }
            self.resetting = false;
        }
    }

    fn reply(&mut self, acknowledge: bool, packet: &[u8]) {
        self.outgoing.clear();
        self.replaying.clear();
        self.reporting = false;
        if acknowledge {
            self.outgoing.push_back(ACK);
        }
        self.outgoing.extend(packet);
        self.completing_packet = (!packet.is_empty()).then(|| packet.to_vec());
    }

    fn invalid(&mut self) {
        self.replaying.clear();
        self.invalid_inputs = self.invalid_inputs.saturating_add(1);
        self.outgoing = [if self.invalid_inputs == 1 {
            RESEND
        } else {
            0xfc
        }]
        .into();
        self.completing_packet = None;
        self.reporting = false;
    }

    pub fn command(&mut self, byte: u8, now: Duration) {
        // BAT and device ID complete reset before another command is accepted.
        if self.resetting {
            return;
        }
        if self.wrapped && byte != RESET && byte != RESET_WRAP_MODE {
            self.reply(false, &[byte]);
            return;
        }
        if byte == RESEND {
            self.replaying = self.last_packet.iter().copied().collect();
            if self.reporting {
                // Resend preserves sampled motion and pending button releases.
                // Restart an interrupted report at its header after the replay.
                self.outgoing = self
                    .completing_packet
                    .as_ref()
                    .unwrap()
                    .iter()
                    .copied()
                    .collect();
            } else {
                self.outgoing.clear();
                self.completing_packet = None;
            }
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
                    self.reply(true, &[]);
                    self.resume_buttons();
                } else {
                    self.invalid();
                }
                return;
            }
        }
        let interrupted_parameter = self.parameter.take();
        // Read Data observes the current counters before the command clears them.
        if byte == READ_DATA {
            let packet = self.motion.read_data();
            self.invalid_inputs = 0;
            self.reply(true, &packet);
            self.reporting = true;
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
                self.resetting = byte == RESET;
                self.reply(true, if self.resetting { &[0xaa, 0] } else { &[] });
            }
            DISABLE => {
                self.enabled = false;
                self.reply(true, &[]);
            }
            ENABLE => {
                self.enabled = true;
                self.sampled_at = now;
                self.reply(true, &[]);
            }
            SET_SAMPLE_RATE | SET_RESOLUTION => {
                self.parameter = Some(if byte == SET_SAMPLE_RATE {
                    Parameter::SampleRate
                } else {
                    Parameter::Resolution
                });
                self.reply(true, &[]);
            }
            IDENTIFY => self.reply(true, &[0]),
            SET_REMOTE_MODE => {
                self.remote = true;
                self.reply(true, &[]);
            }
            SET_WRAP_MODE => {
                self.wrapped = true;
                self.enabled = false;
                self.reply(true, &[]);
            }
            RESET_WRAP_MODE => {
                self.wrapped = false;
                self.reply(true, &[]);
            }
            SET_STREAM_MODE => {
                self.remote = false;
                self.sampled_at = now;
                self.reply(true, &[]);
            }
            STATUS => {
                let buttons = self.motion.buttons;
                let status = (u8::from(self.remote) << 6)
                    | (u8::from(self.enabled) << 5)
                    | (u8::from(self.scaling) << 4)
                    | ((buttons & 1) << 2)
                    | ((buttons & 2) >> 1)
                    | ((buttons & 4) >> 1);
                self.reply(true, &[status, self.resolution, self.sample_rate]);
            }
            RESET_SCALING | SET_SCALING => {
                self.scaling = byte == SET_SCALING;
                self.reply(true, &[]);
            }
            _ => {
                self.parameter = interrupted_parameter;
                self.invalid();
                self.resume_buttons();
                return;
            }
        }
        self.invalid_inputs = 0;
        self.resume_buttons();
    }

    fn resume_buttons(&mut self) {
        if self.streaming() {
            self.motion.rescan_buttons();
        }
    }

    pub fn deadline(&self) -> Option<Duration> {
        (self.streaming()
            && self.outgoing.is_empty()
            && self.replaying.is_empty()
            && self.motion.pending())
        .then(|| {
            self.sampled_at.saturating_add(Duration::from_nanos(
                1_000_000_000_u64.div_ceil(u64::from(self.sample_rate)),
            ))
        })
    }

    pub fn sample(&mut self, now: Duration) {
        if self.deadline().is_some_and(|deadline| now >= deadline) {
            let packet = self.motion.packet(self.scaling);
            self.reply(false, &packet);
            self.reporting = true;
        }
    }
}
