//! Firmware text services shared by INT 10h and the DOS console.

mod bell;

use crate::screen::{self, PageWriter};
use relocade_machine::{real_mode_interrupt::interrupt, Clock, MemoryBus, TimerHandle};
use std::rc::Rc;

/// Teletype output on a machine's memory and I/O bus.
///
/// Keep one shared owner for BIOS interrupt services and console writes. Calls
/// are asynchronous so controls can wait while the execution driver runs devices.
/// Ordinary characters finish without yielding. BEL replaces PIT channel 2 with
/// a roughly 1 kHz tone for 100 ms. Completion or cancellation disables the speaker;
/// previous timer programming and playback are not restored.
pub struct Teletype<C = TimerHandle> {
    pub(crate) memory: MemoryBus,
    clock: Rc<C>,
}

impl<C: Clock> Teletype<C> {
    /// Binds initialized BIOS memory to the clock shared by the machine devices.
    pub fn new(memory: MemoryBus, clock: Rc<C>) -> Self {
        Self { memory, clock }
    }

    /// Writes guest-encoded bytes to the active page using BIOS teletype behavior.
    /// Text cells retain their attributes; graphics glyphs use foreground color 7.
    /// This updates guest video memory and BIOS cursor data without a host stream.
    pub async fn write_text(&self, bytes: &[u8]) {
        for &byte in bytes {
            let _ = self.write_character(byte, 7).await;
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
        let mut writer = PageWriter::new(&mut memory, self.memory.io(), page)?;
        self.write_to_page(&mut writer, character, color, false)
            .await;
        writer.commit_cursor();
        Some(())
    }

    pub(crate) async fn write_to_page(
        &self,
        writer: &mut PageWriter<'_, '_>,
        character: u8,
        color: u8,
        with_attribute: bool,
    ) {
        match character {
            7 => self.bell().await,
            _ => writer.write_character(character, color, with_attribute),
        }
    }
}
