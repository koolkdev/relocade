//! Rust access to the console shared with guest DOS and BIOS services.

mod line;
mod output;
pub(crate) use line::edit_line;
pub(crate) use output::CharacterOutput;

use crate::stdio::Stdio;
use relocade_machine::MemoryBus;

/// A borrowed console on the execution owner. Rust console operations target the
/// screen directly, independent of a process's redirected standard handles.
#[derive(Clone, Copy)]
pub struct Console<'a> {
    pub(crate) stdio: &'a Stdio,
    pub(crate) memory: &'a MemoryBus,
}

impl Console<'_> {
    /// Writes guest-encoded bytes, with the same tab expansion, cursor movement
    /// and scrolling as cooked DOS output. Use CRLF for a new line. This writes
    /// video state directly without adding a duplicate host stdout byte stream.
    pub fn write(&self, bytes: &[u8]) {
        let output = self.screen();
        for &byte in bytes {
            output.write_cooked(byte);
        }
    }

    /// Reads and echoes an edited line of at most 254 bytes. Shares the DOS line
    /// editor (including Backspace), then completes CRLF and excludes the ending
    /// CR from the returned bytes. There must be only one console/guest input reader.
    pub async fn read_line(&self) -> Vec<u8> {
        let mut line = edit_line(
            async || Ok(self.stdio.input.read_character().await),
            255,
            self.screen(),
        )
        .await
        .expect("keyboard console reads cannot fail");
        line.pop(); // The shared DOS editor includes its final CR.
        self.write(b"\n");
        line
    }

    pub(crate) fn write_video(&self, bytes: &[u8]) {
        relocade_bios::write_text(&mut self.memory.view(), self.memory.io(), bytes);
    }
}
