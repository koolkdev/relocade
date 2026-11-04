//! BIOS bell policy over the ordinary channel-2 and speaker ports.

use super::Teletype;
use relocade_machine::{Clock, IoBus};
use relocade_pc_speaker::SPEAKER_CONTROL_PORT;
use relocade_pit::{CHANNEL_2_PORT, CONTROL_PORT};
use std::time::Duration;

const BELL_DIVISOR: u16 = 1193; // Approximately 1 kHz from the PC input clock.
const BELL_DURATION: Duration = Duration::from_millis(100);
const SPEAKER_ENABLE: u32 = 3; // Channel-2 gate and speaker data enable.

impl<C: Clock> Teletype<C> {
    pub(super) async fn bell(&self) {
        let io = self.memory.io();
        let control = io.read_port(SPEAKER_CONTROL_PORT, 1);
        io.write_port(SPEAKER_CONTROL_PORT, 1, control & !SPEAKER_ENABLE);
        io.write_port(CONTROL_PORT, 1, 0xb6); // Channel 2, low/high bytes, square wave.
        for byte in BELL_DIVISOR.to_le_bytes() {
            io.write_port(CHANNEL_2_PORT, 1, u32::from(byte));
        }
        io.write_port(SPEAKER_CONTROL_PORT, 1, control | SPEAKER_ENABLE);
        let _tone = Bell(io);
        self.clock.sleep(BELL_DURATION).await;
    }
}

/// A cancelled service must not leave the speaker running.
struct Bell<'a>(&'a IoBus);

impl Drop for Bell<'_> {
    fn drop(&mut self) {
        let control = self.0.read_port(SPEAKER_CONTROL_PORT, 1);
        self.0
            .write_port(SPEAKER_CONTROL_PORT, 1, control & !SPEAKER_ENABLE);
    }
}
