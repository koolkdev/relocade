//! Pending packets, resend and publication to the shared controller latch.

use super::motion::{Motion, Sample};
use crate::protocol::{mouse::DEVICE_ID, ACK, RESEND, SELF_TEST_PASSED};

pub(super) struct Transmission {
    pending: Option<Packet>,
    replay: Option<usize>,
    last_payload: Vec<u8>,
}

struct Packet {
    payload: Vec<u8>,
    position: usize,
    acknowledge: bool,
    kind: PacketKind,
}

enum PacketKind {
    Reply,
    Error,
    Reset,
    Stream { scaling: bool },
    ReadData,
    Motion(Sample),
}

impl Packet {
    fn new(acknowledge: bool, payload: &[u8], kind: PacketKind) -> Self {
        Self {
            payload: payload.to_vec(),
            position: 0,
            acknowledge,
            kind,
        }
    }

    fn byte(&self, motion: &Motion) -> Option<u8> {
        if self.acknowledge {
            return Some(ACK);
        }
        // Until the header can publish, the live counters are the only copy.
        match self.kind {
            PacketKind::Stream { scaling } => Some(motion.snapshot(true).packet(scaling)[0]),
            PacketKind::ReadData => Some(motion.snapshot(false).packet(false)[0]),
            _ => self.payload.get(self.position).copied(),
        }
    }

    fn advance(&mut self) {
        if !std::mem::take(&mut self.acknowledge) {
            self.position += 1;
        }
    }
}

impl Transmission {
    pub fn new() -> Self {
        Self {
            pending: None,
            replay: None,
            last_payload: vec![RESEND],
        }
    }

    pub fn idle(&self) -> bool {
        self.pending.is_none() && self.replay.is_none()
    }

    pub fn resetting(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|packet| matches!(packet.kind, PacketKind::Reset))
    }

    pub fn byte(&self, motion: &Motion) -> Option<u8> {
        if let Some(position) = self.replay {
            Some(self.last_payload[position])
        } else {
            self.pending.as_ref()?.byte(motion)
        }
    }

    /// Commit a published byte. Returns true if it started a movement packet.
    pub fn sent(&mut self, motion: &mut Motion) -> bool {
        if let Some(position) = &mut self.replay {
            *position += 1;
            if *position == self.last_payload.len() {
                self.replay = None;
            }
            return false;
        }
        let packet = self.pending.as_mut().expect("a pending output byte");
        if !packet.acknowledge
            && matches!(
                packet.kind,
                PacketKind::Stream { .. } | PacketKind::ReadData
            )
        {
            let sample = motion.sample(matches!(packet.kind, PacketKind::Stream { .. }));
            let scaling = matches!(packet.kind, PacketKind::Stream { scaling: true });
            packet.payload = sample.packet(scaling).to_vec();
            packet.kind = PacketKind::Motion(sample);
        }
        let started = matches!(packet.kind, PacketKind::Motion(_)) && packet.position == 0;
        packet.advance();
        if packet.byte(motion).is_none() {
            let packet = self.pending.take().unwrap();
            if !matches!(packet.kind, PacketKind::Error) && !packet.payload.is_empty() {
                self.last_payload = packet.payload;
            }
            if let PacketKind::Motion(sample) = packet.kind {
                motion.report_sent(sample);
            }
        }
        started
    }

    fn replace(&mut self, packet: Packet) {
        self.pending = Some(packet);
        self.replay = None;
    }

    pub fn reply(&mut self, acknowledge: bool, payload: &[u8]) {
        self.replace(Packet::new(acknowledge, payload, PacketKind::Reply));
    }

    pub fn invalid(&mut self, byte: u8) {
        self.replace(Packet::new(false, &[byte], PacketKind::Error));
    }

    pub fn reset(&mut self) {
        self.replace(Packet::new(
            true,
            &[SELF_TEST_PASSED, DEVICE_ID],
            PacketKind::Reset,
        ));
    }

    pub fn stream(&mut self, scaling: bool) {
        self.replace(Packet::new(false, &[], PacketKind::Stream { scaling }));
    }

    pub fn read_data(&mut self, motion: &mut Motion) {
        let interrupted = match self.pending.take().map(|packet| packet.kind) {
            Some(PacketKind::Motion(sample)) => Some(sample),
            _ => None,
        };
        motion.read_data(interrupted);
        self.replace(Packet::new(true, &[], PacketKind::ReadData));
    }

    pub fn resend(&mut self) {
        self.replay = Some(0);
        if let Some(packet) = &mut self.pending {
            if matches!(
                packet.kind,
                PacketKind::Stream { .. } | PacketKind::ReadData | PacketKind::Motion(_)
            ) {
                // Keep the report's unscaled sample and restart at its header
                // after replay. ACK belongs to the command, not the payload.
                packet.position = 0;
                packet.acknowledge = false;
            } else {
                self.pending = None;
            }
        }
    }
}
