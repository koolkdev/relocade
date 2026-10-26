//! DOS handle slots, sharing admission and open-record lifetime.

use super::{
    devices::DeviceHandle,
    file_handle::{FileHandle, SeekOrigin},
    AccessMode, Device, FileSystem, Files,
};
use crate::{error::DosError, path::ResolvedPath, stdio::OutputStream};
use relocade_bios::Keyboard;
use relocade_vfs::File;
use std::rc::Rc;

// A DOS process starts with 20 handles, including its five standard devices.
pub(super) const HANDLE_COUNT: usize = 20;

#[derive(Clone, Copy)]
pub(crate) enum HandleType {
    File,
    Device(Device),
}

#[derive(Clone)]
pub(super) enum Handle {
    // Separate opens have separate cursors; duplicated handles share this record.
    File(FileHandle),
    Device(Rc<DeviceHandle>),
}

impl Default for Files {
    fn default() -> Self {
        let mut handles: [Option<Handle>; HANDLE_COUNT] = Default::default();
        for (slot, device) in handles.iter_mut().zip([
            Device::Console(OutputStream::Stdout),
            Device::Console(OutputStream::Stdout),
            Device::Console(OutputStream::Stderr),
            Device::Auxiliary,
            Device::Printer,
        ]) {
            *slot = Some(Handle::Device(Rc::new(DeviceHandle::new(
                device,
                AccessMode::ReadWrite,
            ))));
        }
        Self {
            handles,
            search_directories: Vec::new(),
        }
    }
}

impl Files {
    pub(super) fn is_open(&self, file: &File) -> bool {
        // DOS sharing keeps an open file's name alive until its last handle closes.
        self.handles
            .iter()
            .flatten()
            .any(|handle| matches!(handle, Handle::File(open) if open.same_file(file)))
    }

    pub(super) fn open_file(
        &mut self,
        file: File,
        path: ResolvedPath,
        access: AccessMode,
        sharing: u8,
    ) -> Result<u16, DosError> {
        let entry = FileHandle::new(file, path, access, sharing)?;
        if self
            .handles
            .iter()
            .flatten()
            .any(|handle| matches!(handle, Handle::File(open) if open.conflicts_with(&entry)))
        {
            return Err(DosError::AccessDenied);
        }
        let handle = self.vacant_handle()?;
        self.handles[handle] = Some(Handle::File(entry));
        Ok(handle as u16)
    }

    pub(super) fn vacant_handle(&self) -> Result<usize, DosError> {
        self.handles
            .iter()
            .position(Option::is_none)
            .ok_or(DosError::TooManyOpenFiles)
    }

    pub(crate) fn handle_type(&self, handle: u16) -> Result<HandleType, DosError> {
        Ok(match self.handle(handle)? {
            Handle::File(_) => HandleType::File,
            Handle::Device(device) => HandleType::Device(device.kind),
        })
    }

    pub(crate) fn readable_handle_type(&self, handle: u16) -> Result<HandleType, DosError> {
        let readable = match self.handle(handle)? {
            Handle::File(file) => file.access().can_read(),
            Handle::Device(device) => device.access.can_read(),
        };
        if !readable {
            return Err(DosError::AccessDenied);
        }
        self.handle_type(handle)
    }

    pub(crate) fn writable_handle_type(&self, handle: u16) -> Result<HandleType, DosError> {
        let writable = match self.handle(handle)? {
            Handle::File(file) => file.access().can_write(),
            Handle::Device(device) => device.access.can_write(),
        };
        if !writable {
            return Err(DosError::AccessDenied);
        }
        self.handle_type(handle)
    }

    pub(crate) fn read(&self, handle: u16, buffer: &mut [u8]) -> Result<usize, DosError> {
        match self.handle(handle)? {
            Handle::File(file) => file.read(buffer),
            Handle::Device(device) => {
                if !device.access.can_read() || !matches!(device.kind, Device::Null) {
                    return Err(DosError::AccessDenied);
                }
                if !buffer.is_empty() {
                    device.set_eof(true);
                }
                Ok(0)
            }
        }
    }

    pub(crate) fn write(&self, handle: u16, bytes: &[u8]) -> Result<usize, DosError> {
        match self.handle(handle)? {
            Handle::File(file) => file.write(bytes),
            Handle::Device(device) => {
                if !device.access.can_write() || !matches!(device.kind, Device::Null) {
                    return Err(DosError::AccessDenied);
                }
                // Even a zero-byte handle write clears EOF on this record and its duplicates.
                device.set_eof(false);
                Ok(bytes.len())
            }
        }
    }

    pub(crate) fn seek(&self, handle: u16, origin: u8, offset: u32) -> Result<u32, DosError> {
        let open = self.handle(handle)?;
        let origin = match origin {
            0 => SeekOrigin::Start,
            1 => SeekOrigin::Current,
            2 => SeekOrigin::End,
            _ => return Err(DosError::InvalidFunction),
        };
        match open {
            Handle::File(file) => file.seek(origin, offset),
            Handle::Device(_) => Ok(0),
        }
    }

    pub(crate) fn close(
        &mut self,
        filesystem: &mut FileSystem,
        handle: u16,
    ) -> Result<(), DosError> {
        let open = self
            .handles
            .get_mut(usize::from(handle))
            .and_then(Option::take)
            .ok_or(DosError::InvalidHandle)?;
        Self::commit_handle(filesystem, open);
        Ok(())
    }

    pub(crate) fn commit(&self, filesystem: &mut FileSystem, handle: u16) -> Result<(), DosError> {
        let open = self.handle(handle)?.clone();
        Self::commit_handle(filesystem, open);
        Ok(())
    }

    fn commit_handle(filesystem: &mut FileSystem, handle: Handle) {
        if let Handle::File(file) = handle {
            file.commit(filesystem);
        }
    }

    pub(crate) fn duplicate(&mut self, source: u16) -> Result<u16, DosError> {
        let open = self.handle(source)?.clone();
        let destination = self.vacant_handle()?;
        self.handles[destination] = Some(open);
        Ok(destination as u16)
    }

    pub(crate) fn force_duplicate(
        &mut self,
        filesystem: &mut FileSystem,
        source: u16,
        destination: u16,
    ) -> Result<(), DosError> {
        let open = self.handle(source)?.clone();
        if source == destination {
            return Ok(());
        }
        let slot = self
            .handles
            .get_mut(usize::from(destination))
            .ok_or(DosError::InvalidHandle)?;
        if let Some(replaced) = slot.replace(open) {
            Self::commit_handle(filesystem, replaced);
        }
        Ok(())
    }

    pub(crate) fn device_information(&self, handle: u16) -> Result<u16, DosError> {
        Ok(match self.handle(handle)? {
            Handle::File(file) => file.information(),
            Handle::Device(device) => device.information(),
        })
    }

    /// DOS status queries ask the device, not the buffered handle-read line or EOF bit.
    pub(crate) fn input_available(
        &self,
        keyboard: &Keyboard,
        handle: u16,
    ) -> Result<bool, DosError> {
        Ok(match self.handle(handle)? {
            Handle::File(file) => file.input_available(),
            Handle::Device(device) => match device.kind {
                Device::Console(_) => keyboard.has_input(),
                Device::Null => true,
                Device::Auxiliary | Device::Printer => false,
            },
        })
    }

    pub(crate) fn output_ready(&self, handle: u16) -> Result<bool, DosError> {
        Ok(match self.handle(handle)? {
            // DOS reports files ready even when opened for reading only.
            Handle::File(_) => true,
            Handle::Device(device) => match device.kind {
                Device::Console(_) | Device::Null => true,
                // These standard device slots have no attached host streams yet.
                Device::Auxiliary | Device::Printer => false,
            },
        })
    }

    pub(crate) fn close_all(self, filesystem: &mut FileSystem) {
        for handle in self.handles.into_iter().flatten() {
            Self::commit_handle(filesystem, handle);
        }
    }

    pub(super) fn handle(&self, handle: u16) -> Result<&Handle, DosError> {
        self.handles
            .get(usize::from(handle))
            .and_then(Option::as_ref)
            .ok_or(DosError::InvalidHandle)
    }
}
