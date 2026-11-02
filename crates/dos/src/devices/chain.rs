//! Ordered registration and the identity retained by an open device handle.

use super::{attributes, CharacterDriver, DriverFuture};
use crate::DosError;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Default)]
pub(crate) struct DeviceChain(Rc<RefCell<Option<Rc<CharacterDevice>>>>);

/// A named DOS character device with a fixed driver binding. Retaining this
/// device keeps its I/O connected after another driver is installed for its name.
pub struct CharacterDevice {
    name: [u8; 8],
    attributes: u16,
    driver: Box<dyn CharacterDriver>,
    next: Option<Rc<CharacterDevice>>,
}

impl DeviceChain {
    pub fn install(&self, driver: impl CharacterDriver + 'static) {
        let name = driver.name();
        let attributes = driver.attributes();
        assert!(
            attributes & attributes::CHARACTER != 0,
            "DOS character driver"
        );
        let mut head = self.0.borrow_mut();
        *head = Some(Rc::new(CharacterDevice {
            name,
            attributes,
            driver: Box::new(driver),
            next: head.take(),
        }));
    }

    pub fn find(&self, name: &[u8]) -> Option<Rc<CharacterDevice>> {
        let mut entry = self.0.borrow().clone();
        while let Some(device) = entry {
            if device.name().trim_ascii_end().eq_ignore_ascii_case(name) {
                return Some(device);
            }
            entry = device.next.clone();
        }
        None
    }

    pub fn console(&self) -> Rc<CharacterDevice> {
        self.find(b"CON").expect("DOS installs a console device")
    }
}

impl CharacterDevice {
    /// The DOS name, padded to eight bytes, captured at installation.
    pub fn name(&self) -> [u8; 8] {
        self.name
    }

    /// DOS device attributes captured at installation, also reported by IOCTL.
    pub fn attributes(&self) -> u16 {
        self.attributes
    }

    /// Whether a character read can complete immediately.
    pub fn input_available(&self) -> DriverFuture<'_, bool> {
        self.driver.input_available()
    }

    /// Discards queued input.
    pub fn flush_input(&self) -> DriverFuture<'_, ()> {
        self.driver.flush_input()
    }

    /// Takes a character without waiting, or returns None when input is pending.
    pub async fn try_read_character(&self) -> Result<Option<u8>, DosError> {
        match self.peek_character().await? {
            Some(_) => self.read_character().await.map(Some),
            None => Ok(None),
        }
    }

    /// Reports the next character without consuming it or waiting for input.
    pub fn peek_character(&self) -> DriverFuture<'_, Option<u8>> {
        self.driver.peek_character()
    }

    /// Reads bytes without DOS line editing and returns the actual transfer count.
    pub fn read<'a>(&'a self, bytes: &'a mut [u8]) -> DriverFuture<'a, usize> {
        self.driver.read(bytes)
    }

    /// DOS single-character input. An EOF transfer produces zero, as with NUL;
    /// use `read` when the caller needs to distinguish EOF from a zero byte.
    pub async fn read_character(&self) -> Result<u8, DosError> {
        let mut byte = [0];
        let count = self.read(&mut byte).await?;
        Ok(if count == 0 { 0 } else { byte[0] })
    }

    /// Whether the device is ready for output.
    pub fn output_ready(&self) -> DriverFuture<'_, bool> {
        self.driver.output_ready()
    }

    /// Writes bytes without DOS cooked-output processing and returns the actual count.
    pub fn write<'a>(&'a self, bytes: &'a [u8]) -> DriverFuture<'a, usize> {
        self.driver.write(bytes)
    }

    pub(crate) fn is_console(&self) -> bool {
        self.attributes & 3 == 3
    }

    pub(crate) fn is_null(&self) -> bool {
        self.attributes & 4 != 0
    }
}
