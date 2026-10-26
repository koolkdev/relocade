//! Character input through owned file/device records and the console.

use super::{
    devices::OpenDevice,
    handles::{OpenFile, OpenHandle},
    Device, Files, HandleType,
};
use crate::error::DosError;
use relocade_bios::Keyboard;
use std::{cell::RefCell, future::pending, rc::Rc};

/// Retains the input source selected when the service starts. Replacing a DOS
/// handle does not redirect a read that is already waiting.
pub(crate) struct CharacterInput {
    source: Source,
}

enum Source {
    Console {
        keyboard: Keyboard,
        device: Rc<OpenDevice>,
    },
    File(Rc<RefCell<OpenFile>>),
    Null,
}

impl CharacterInput {
    pub(crate) fn try_read_character(&self) -> Result<Option<u8>, DosError> {
        match &self.source {
            Source::Console { keyboard, .. } => Ok(keyboard.try_read_character()),
            Source::Null => Ok(Some(0)),
            Source::File(file) => {
                let mut byte = [0];
                Ok((file.borrow_mut().read(&mut byte)? != 0).then_some(byte[0]))
            }
        }
    }

    pub(crate) async fn read_character(&self) -> Result<u8, DosError> {
        if let Source::Console { keyboard, .. } = &self.source {
            return Ok(keyboard.read_character().await);
        }
        match self.try_read_character()? {
            Some(byte) => Ok(byte),
            // File growth has no notification source; EOF cannot consume console input.
            None => pending().await,
        }
    }

    pub(crate) async fn read_line_character(&self) -> Result<u8, DosError> {
        // NUL returns zero forever, but cannot complete a line. Do not spin.
        if matches!(self.source, Source::Null) {
            return pending().await;
        }
        self.read_character().await
    }

    pub(crate) fn console(&self) -> Option<&OpenDevice> {
        match &self.source {
            Source::Console { device, .. } => Some(device),
            _ => None,
        }
    }
}

impl Files {
    /// Flush the device queue without changing file cursors, buffered lines or EOF.
    pub(crate) fn flush_input(&self, keyboard: &Keyboard, handle: u16) -> Result<(), DosError> {
        match self.handle_type(handle)? {
            HandleType::Device(Device::Console(_)) => keyboard.discard_queued(),
            HandleType::File | HandleType::Device(_) => {}
        }
        Ok(())
    }

    pub(crate) fn character_input(
        &self,
        keyboard: &Keyboard,
        handle: u16,
    ) -> Result<CharacterInput, DosError> {
        self.readable_handle_type(handle)?;
        let source = match self.handle(handle)? {
            OpenHandle::File(file) => Source::File(Rc::clone(file)),
            OpenHandle::Device(device) => match device.kind {
                Device::Console(_) => Source::Console {
                    keyboard: keyboard.clone(),
                    device: Rc::clone(device),
                },
                Device::Null => Source::Null,
                Device::Auxiliary | Device::Printer => return Err(DosError::AccessDenied),
            },
        };
        Ok(CharacterInput { source })
    }

    pub(crate) fn read_character(
        &self,
        keyboard: &Keyboard,
        handle: u16,
    ) -> Result<Option<u8>, DosError> {
        self.character_input(keyboard, handle)?.try_read_character()
    }
}
