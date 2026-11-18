//! DOS file handles owned by the native prompt.

use crate::{
    devices::CharacterDevice,
    files::{CreateMode, Files, Handle},
    state::DosState,
    DosError,
};
use std::{cell::RefCell, rc::Rc};

/// How the prompt opens a redirected output file. Both modes write to a named
/// character device through its installed driver.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputMode {
    Truncate,
    Append,
}

pub(super) struct PromptFiles {
    state: Rc<DosState>,
    table: RefCell<Files>,
}

/// An open prompt handle. Dropping it closes the slot and commits DOS metadata.
pub struct ConsoleFile {
    owner: Rc<PromptFiles>,
    slot: u16,
    handle: Handle,
}

impl PromptFiles {
    pub(super) fn new(state: Rc<DosState>) -> Rc<Self> {
        Rc::new(Self {
            table: RefCell::new(Files::new(state.devices().clone(), state.stdio())),
            state,
        })
    }

    pub(super) fn open_input_file(self: &Rc<Self>, path: &[u8]) -> Result<ConsoleFile, DosError> {
        let open = {
            let mut table = self.table.borrow_mut();
            // Read-only, deny writes while this queued input is open.
            let slot = table.open(&self.state.filesystem(), path, 0x20)?;
            self.retain(&table, slot)
        };
        if open.device().is_some() {
            return Err(DosError::AccessDenied);
        }
        Ok(open)
    }

    pub(super) fn open_output(
        self: &Rc<Self>,
        path: &[u8],
        mode: OutputMode,
    ) -> Result<ConsoleFile, DosError> {
        let open = {
            let mut table = self.table.borrow_mut();
            let mut fs = self.state.filesystem();
            let slot = match mode {
                OutputMode::Truncate => {
                    table.create(&mut fs, path, 0, CreateMode::TruncateExisting)?
                }
                OutputMode::Append => match table.open(&fs, path, 1) {
                    Ok(slot) => slot,
                    Err(DosError::FileNotFound) => {
                        table.create(&mut fs, path, 0, CreateMode::FailIfExists)?
                    }
                    Err(error) => return Err(error),
                },
            };
            self.retain(&table, slot)
        };
        if mode == OutputMode::Append {
            self.table.borrow().seek(open.slot, 2, 0)?;
        }
        Ok(open)
    }

    fn retain(self: &Rc<Self>, table: &Files, slot: u16) -> ConsoleFile {
        ConsoleFile {
            owner: self.clone(),
            slot,
            handle: table.handle(slot).expect("just opened DOS handle").clone(),
        }
    }
}

impl ConsoleFile {
    /// Returns the actual driver binding when the path opened a character device.
    pub fn device(&self) -> Option<&CharacterDevice> {
        match &self.handle {
            Handle::Device(handle) => Some(&handle.device),
            Handle::File(_) => None,
        }
    }

    /// Reads from the current DOS cursor.
    pub async fn read(&self, bytes: &mut [u8]) -> Result<usize, DosError> {
        self.handle.check_read_access()?;
        self.handle.read(bytes).await
    }

    /// Writes the entire buffer, including when a driver accepts a short write.
    /// Explicit prompt writes to CON go directly to the driver.
    pub async fn write_all(&self, bytes: &[u8]) -> Result<(), DosError> {
        if bytes.is_empty() {
            return Ok(());
        }
        match &self.handle {
            Handle::Device(handle) => handle.device.write_all(bytes).await,
            Handle::File(file) => {
                if file.write(bytes)? == bytes.len() {
                    Ok(())
                } else {
                    Err(DosError::WriteFault)
                }
            }
        }
    }
}

impl Drop for ConsoleFile {
    fn drop(&mut self) {
        self.owner
            .table
            .borrow_mut()
            .close(&mut self.owner.state.filesystem(), self.slot)
            .expect("prompt owns its DOS handle");
    }
}
