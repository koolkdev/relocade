//! Native prompt I/O and the line editing shared with guest DOS services.

mod line;
mod output;
pub(crate) use line::{edit_line, LineEditor};
pub(crate) use output::CharacterOutput;

use crate::{devices::dos_bytes, stdio::Stdio};
use relocade_bios::PhysicalKeyboard;
use relocade_frontend::keyboard::KeyboardEvents;
use relocade_machine::MemoryBus;

/// A native prompt with its own physical input and the PC's BIOS video state.
/// Reads consume application events directly. Guest IRQ handlers, DOS drivers
/// and redirected handles do not participate; guest keyboard RAM is untouched.
/// The exclusive DOS borrow keeps guest execution separate from this prompt.
pub struct NativeConsole<'a> {
    memory: MemoryBus,
    stdio: &'a Stdio,
    events: KeyboardEvents,
    keyboard: PhysicalKeyboard,
}

impl<'a> NativeConsole<'a> {
    pub(crate) fn new(memory: MemoryBus, stdio: &'a Stdio, events: KeyboardEvents) -> Self {
        Self {
            memory,
            stdio,
            events,
            keyboard: PhysicalKeyboard::default(),
        }
    }

    /// Writes guest-encoded text using cooked tab stops and BIOS cursor/scrolling
    /// behavior. Use CRLF for a new line. Output is synchronous and screen-only.
    pub fn write(&self, bytes: &[u8]) {
        write(self.stdio, &self.memory, bytes);
    }

    /// Reads and echoes an edited line of at most 254 bytes, excluding its final
    /// CRLF. An empty input channel waits; a closed channel returns None.
    /// Cancelling drops the partial line, retaining the translator's held keys.
    pub async fn read_line(&mut self) -> Option<Vec<u8>> {
        let mut pending = None;
        let mut line = edit_line::<()>(
            async || {
                if let Some(byte) = pending.take() {
                    return Ok(byte);
                }
                loop {
                    let event = self.events.recv().await.map_err(|_| ())?;
                    if let Some(key) = self.keyboard.translate(event) {
                        let mut bytes = dos_bytes(key);
                        let byte = bytes.next().expect("a keystroke contains a character");
                        pending = bytes.next();
                        return Ok(byte);
                    }
                }
            },
            255,
            async |byte| {
                write(self.stdio, &self.memory, &[byte]);
                Ok(())
            },
        )
        .await
        .ok()?;
        line.pop(); // The shared DOS editor includes its final CR.
        self.write(b"\n");
        Some(line)
    }
}

fn write(stdio: &Stdio, memory: &MemoryBus, bytes: &[u8]) {
    let mut view = memory.view();
    for &byte in bytes {
        let (byte, count) = stdio.cook_character(byte);
        relocade_bios::write_text(&mut view, memory.io(), &[byte; 8][..usize::from(count)]);
    }
}
