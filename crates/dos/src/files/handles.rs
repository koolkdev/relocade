//! DOS handle slots, sharing admission and open-record lifetime.

use super::{
    devices::DeviceHandle,
    file_handle::{FileHandle, SeekOrigin},
    AccessMode, FileSystem, Files,
};
use crate::{devices::DeviceChain, error::DosError, stdio::OutputStream};
use relocade_vfs::File;
use std::rc::Rc;

// A DOS process starts with 20 handles, including its five standard devices.
pub(super) const HANDLE_COUNT: usize = 20;

#[derive(Clone)]
pub(crate) enum Handle {
    // Separate opens have separate cursors; duplicated handles share this record.
    File(FileHandle),
    Device(Rc<DeviceHandle>),
}

impl Files {
    pub(crate) fn new(devices: DeviceChain) -> Self {
        let mut handles: [Option<Handle>; HANDLE_COUNT] = Default::default();
        for (slot, (name, stream)) in handles.iter_mut().zip([
            (b"CON", OutputStream::Stdout),
            (b"CON", OutputStream::Stdout),
            (b"CON", OutputStream::Stderr),
            (b"AUX", OutputStream::Stdout),
            (b"PRN", OutputStream::Stdout),
        ]) {
            *slot = Some(Handle::Device(Rc::new(DeviceHandle::new(
                devices.find(name).expect("standard DOS device"),
                stream,
                AccessMode::ReadWrite,
            ))));
        }
        Self {
            devices,
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

    pub(super) fn insert_file(&mut self, entry: FileHandle) -> Result<u16, DosError> {
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

    /// Resolves the current slot once; the returned record survives later slot changes.
    pub(crate) fn readable_handle(&self, number: u16) -> Result<Handle, DosError> {
        let handle = self.handle(number)?;
        if !handle.access().can_read() {
            return Err(DosError::AccessDenied);
        }
        Ok(handle.clone())
    }

    pub(crate) fn writable_handle(&self, number: u16) -> Result<Handle, DosError> {
        let handle = self.handle(number)?;
        if !handle.access().can_write() {
            return Err(DosError::AccessDenied);
        }
        Ok(handle.clone())
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

    pub(crate) fn close_all(self, filesystem: &mut FileSystem) {
        for handle in self.handles.into_iter().flatten() {
            Self::commit_handle(filesystem, handle);
        }
    }

    pub(crate) fn handle(&self, handle: u16) -> Result<&Handle, DosError> {
        self.handles
            .get(usize::from(handle))
            .and_then(Option::as_ref)
            .ok_or(DosError::InvalidHandle)
    }
}

impl Handle {
    fn access(&self) -> AccessMode {
        match self {
            Self::File(file) => file.access(),
            Self::Device(device) => device.access,
        }
    }
}
