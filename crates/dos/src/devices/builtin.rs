//! BIOS-backed console and the built-in DOS sink/unavailable drivers.

use super::{attributes, input::ConsoleInput, CharacterDriver, DriverFuture};
use relocade_bios::{Keyboard, Teletype};
use std::rc::Rc;

pub(crate) struct BiosConsole {
    teletype: Rc<Teletype>,
    input: ConsoleInput,
}

impl BiosConsole {
    pub fn new(teletype: Rc<Teletype>, keyboard: Keyboard) -> Self {
        Self {
            teletype,
            input: ConsoleInput::new(keyboard),
        }
    }
}

impl CharacterDriver for BiosConsole {
    fn name(&self) -> [u8; 8] {
        *b"CON     "
    }
    fn attributes(&self) -> u16 {
        attributes::CONSOLE
    }

    fn input_available(&self) -> DriverFuture<'_, bool> {
        Box::pin(async move { Ok(self.input.peek_character().is_some()) })
    }
    fn flush_input(&self) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            self.input.flush_input();
            Ok(())
        })
    }
    fn peek_character(&self) -> DriverFuture<'_, Option<u8>> {
        Box::pin(async move { Ok(self.input.peek_character()) })
    }
    fn read<'a>(&'a self, bytes: &'a mut [u8]) -> DriverFuture<'a, usize> {
        Box::pin(async move {
            self.input.read(bytes).await;
            Ok(bytes.len())
        })
    }
    fn output_ready(&self) -> DriverFuture<'_, bool> {
        Box::pin(async move { Ok(true) })
    }
    fn write<'a>(&'a self, bytes: &'a [u8]) -> DriverFuture<'a, usize> {
        Box::pin(async move {
            self.teletype.write_text(bytes).await;
            Ok(bytes.len())
        })
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
    fn input_available(&self) -> DriverFuture<'_, bool> {
        Box::pin(async move { Ok(true) })
    }
    fn peek_character(&self) -> DriverFuture<'_, Option<u8>> {
        Box::pin(async move { Ok(Some(0)) })
    }
    fn read<'a>(&'a self, _bytes: &'a mut [u8]) -> DriverFuture<'a, usize> {
        Box::pin(async { Ok(0) })
    }
    fn output_ready(&self) -> DriverFuture<'_, bool> {
        Box::pin(async move { Ok(true) })
    }
    fn write<'a>(&'a self, bytes: &'a [u8]) -> DriverFuture<'a, usize> {
        Box::pin(async move { Ok(bytes.len()) })
    }
}

pub(crate) struct Unavailable(pub [u8; 8]);

impl CharacterDriver for Unavailable {
    fn name(&self) -> [u8; 8] {
        self.0
    }
}
