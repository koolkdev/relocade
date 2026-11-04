//! Firmware text services shared by INT 10h and the DOS console.

use crate::screen::{self, Output};
use relocade_machine::{real_mode_interrupt::interrupt, MemoryBus};

/// Teletype output on a machine's memory and I/O bus.
///
/// Keep one shared owner for BIOS interrupt services and console writes. Calls
/// are asynchronous so controls can wait while the execution driver runs devices.
/// Ordinary characters finish without yielding.
pub struct Teletype {
    pub(crate) memory: MemoryBus,
}

impl Teletype {
    /// Binds output to initialized BIOS memory and its connected video device.
    pub fn new(memory: MemoryBus) -> Self {
        Self { memory }
    }

    /// Writes guest-encoded bytes to the active page using BIOS teletype behavior.
    /// Text cells retain their attributes; graphics glyphs use foreground color 7.
    /// This updates guest video memory and BIOS cursor data without a host stream.
    pub async fn write_text(&self, bytes: &[u8]) {
        let mut memory = self.memory.view();
        let page = screen::active_page(&memory);
        if let Some(mut output) = Output::new(&mut memory, self.memory.io(), page) {
            for &byte in bytes {
                output.write_character(byte, 7, false);
            }
            output.commit_cursor();
        }
    }

    /// Writes one teletype character to the active page and commits its cursor.
    /// Text keeps cell attributes; graphics uses color. Controls, wrapping and
    /// scrolling match INT 10h/AH=0Eh. Invalid firmware page state returns None.
    #[interrupt(args(character = al, color = bl),
        returns { Some(()) => {}, None => crate::interrupt::unsupported },
    )]
    pub async fn write_character(&self, character: u8, color: u8) -> Option<()> {
        let mut memory = self.memory.view();
        let page = screen::active_page(&memory);
        let mut output = Output::new(&mut memory, self.memory.io(), page)?;
        output.write_character(character, color, false);
        output.commit_cursor();
        Some(())
    }
}
