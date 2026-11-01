//! Ordered registration and the identity retained by an open device handle.

use super::{attributes, CharacterDriver, ReadFuture};
use crate::error::DosError;
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
    pub fn input_available(&self) -> bool {
        self.driver.input_available()
    }

    /// Discards queued input.
    pub fn flush_input(&self) {
        self.driver.flush_input();
    }

    /// Takes a character without waiting, or returns None when input is pending.
    pub fn try_read_character(&self) -> Result<Option<u8>, DosError> {
        self.driver.try_read_character()
    }

    /// Waits for one character using this device's retained driver.
    pub fn read_character(&self) -> ReadFuture<'_> {
        self.driver.read_character()
    }

    /// Whether the device is ready for output.
    pub fn output_ready(&self) -> bool {
        self.driver.output_ready()
    }

    /// Writes all supplied bytes without DOS cooked-output processing.
    pub fn write(&self, bytes: &[u8]) -> Result<(), DosError> {
        self.driver.write(bytes)
    }

    pub(crate) fn output_column(&self) -> Option<u8> {
        self.driver.output_column()
    }

    pub(crate) fn is_console(&self) -> bool {
        self.attributes & 3 == 3
    }

    pub(crate) fn is_null(&self) -> bool {
        self.attributes & 4 != 0
    }
}
