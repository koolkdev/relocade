//! Rust access to the console shared with guest DOS and BIOS services.

mod line;
mod output;
pub(crate) use line::edit_line;
pub(crate) use output::CharacterOutput;

use crate::{devices::DeviceChain, stdio::Stdio};

/// A borrowed console on the execution owner. Rust console operations resolve
/// the current CON device, independent of a process's redirected standard handles.
#[derive(Clone, Copy)]
pub struct Console<'a> {
    pub(crate) devices: &'a DeviceChain,
    pub(crate) stdio: &'a Stdio,
}

impl Console<'_> {
    /// Writes guest-encoded bytes, with the same tab expansion, cursor movement
    /// and scrolling as cooked DOS output. Use CRLF for a new line. This writes
    /// through CON without adding a duplicate host stdout byte stream.
    pub fn write(&self, bytes: &[u8]) {
        let output = self.direct_output(self.devices.console());
        for &byte in bytes {
            output.write_cooked(byte);
        }
    }

    /// Reads and echoes an edited line of at most 254 bytes. Shares the DOS line
    /// editor (including Backspace), then completes CRLF and excludes the ending
    /// CR from the returned bytes. There must be only one console/guest input reader.
    pub async fn read_line(&self) -> Vec<u8> {
        let device = self.devices.console();
        let output = self.direct_output(device.clone());
        let mut line = edit_line(async || device.read_character().await, 255, output)
            .await
            .expect("keyboard console reads cannot fail");
        line.pop(); // The shared DOS editor includes its final CR.
        device.write(b"\n").expect("console output");
        line
    }
}
