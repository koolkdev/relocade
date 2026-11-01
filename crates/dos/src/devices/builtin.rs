//! BIOS-backed console and the built-in DOS sink/unavailable drivers.

use super::{attributes, CharacterDriver, ReadFuture};
use crate::error::DosError;
use relocade_bios::Keyboard;
use relocade_machine::MemoryBus;

pub(crate) struct BiosConsole {
    memory: MemoryBus,
    keyboard: Keyboard,
}

impl BiosConsole {
    pub fn new(memory: MemoryBus, keyboard: Keyboard) -> Self {
        Self { memory, keyboard }
    }
}

impl CharacterDriver for BiosConsole {
    fn name(&self) -> [u8; 8] {
        *b"CON     "
    }
    fn attributes(&self) -> u16 {
        attributes::CONSOLE
    }

    fn input_available(&self) -> bool {
        self.keyboard.has_input()
    }
    fn flush_input(&self) {
        self.keyboard.discard_queued();
    }
    fn try_read_character(&self) -> Result<Option<u8>, DosError> {
        Ok(self.keyboard.try_read_character())
    }
    fn read_character(&self) -> ReadFuture<'_> {
        Box::pin(async { Ok(self.keyboard.read_character().await) })
    }
    fn output_ready(&self) -> bool {
        true
    }
    fn write(&self, bytes: &[u8]) -> Result<(), DosError> {
        relocade_bios::write_text(&mut self.memory.view(), self.memory.io(), bytes);
        Ok(())
    }
}

pub(crate) struct NullDevice;

impl CharacterDriver for NullDevice {
    fn name(&self) -> [u8; 8] {
        *b"NUL     "
    }
    fn attributes(&self) -> u16 {
        attributes::NULL
    }
    fn input_available(&self) -> bool {
        true
    }
    fn try_read_character(&self) -> Result<Option<u8>, DosError> {
        Ok(Some(0))
    }
    fn read_character(&self) -> ReadFuture<'_> {
        Box::pin(std::future::ready(Ok(0)))
    }
    fn output_ready(&self) -> bool {
        true
    }
    fn write(&self, _bytes: &[u8]) -> Result<(), DosError> {
        Ok(())
    }
}

pub(crate) struct Unavailable(pub [u8; 8]);

impl CharacterDriver for Unavailable {
    fn name(&self) -> [u8; 8] {
        self.0
    }
}
