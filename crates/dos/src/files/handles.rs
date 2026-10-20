//! DOS open handles, shared cursors and I/O.

use super::{
    devices::OpenDevice, AccessMode, CharacterOutput, Device, Files, ARCHIVE_ATTRIBUTE,
    COMPATIBILITY_SHARING,
};
use crate::{error::DosError, path::ResolvedPath, stdio::OutputStream};
use relocade_vfs::File;
use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

// A DOS process starts with 20 handles, including its five standard devices.
pub(super) const HANDLE_COUNT: usize = 20;

#[derive(Clone, Copy)]
pub(crate) enum HandleType {
    File,
    Device(Device),
}

#[derive(Clone)]
pub(super) enum OpenHandle {
    // Separate opens have separate cursors; duplicated handles share this record.
    File(Rc<RefCell<OpenFile>>),
    Device(Rc<OpenDevice>),
}

pub(super) struct OpenFile {
    file: File,
    path: ResolvedPath,
    position: u32,
    access: AccessMode,
    deny_read: bool,
    deny_write: bool,
    compatibility: bool,
    written: bool,
}

impl Files {
    pub(super) fn is_open(&self, file: &File) -> bool {
        // DOS sharing keeps an open file's name alive until its last handle closes.
        self.handles.iter().flatten().any(
            |handle| matches!(handle, OpenHandle::File(open) if open.borrow().file.same_file(file)),
        )
    }

    pub(super) fn open_file(
        &mut self,
        file: File,
        path: ResolvedPath,
        access: AccessMode,
        sharing: u8,
    ) -> Result<u16, DosError> {
        if file.len() > u64::from(u32::MAX) {
            return Err(DosError::AccessDenied);
        }
        let entry = OpenFile {
            file,
            path,
            position: 0,
            access,
            deny_read: matches!(sharing, 0x10 | 0x30),
            deny_write: matches!(sharing, 0x10 | 0x20),
            compatibility: sharing == COMPATIBILITY_SHARING,
            written: false,
        };
        let read_only = entry.file.is_read_only();
        if self.handles.iter().flatten().any(|handle| {
            let OpenHandle::File(open) = handle else {
                return false;
            };
            let open = open.borrow();
            open.file.same_file(&entry.file)
                && ((entry.access.can_read() && open.deny_read)
                    || (entry.access.can_write() && open.deny_write)
                    // New deny-read/write modes exempt this process's existing accesses.
                    || (entry.deny_read && entry.deny_write)
                    // Compatibility and explicit sharing may mix only for read-only files.
                    || (open.compatibility != entry.compatibility
                        && !(read_only && !open.access.can_write() && !entry.access.can_write())))
        }) {
            return Err(DosError::AccessDenied);
        }
        let handle = self.vacant_handle()?;
        self.handles[handle] = Some(OpenHandle::File(Rc::new(RefCell::new(entry))));
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
            OpenHandle::File(_) => HandleType::File,
            OpenHandle::Device(device) => HandleType::Device(device.kind),
        })
    }

    pub(crate) fn readable_handle_type(&self, handle: u16) -> Result<HandleType, DosError> {
        let readable = match self.handle(handle)? {
            OpenHandle::File(file) => file.borrow().access.can_read(),
            OpenHandle::Device(device) => device.access.can_read(),
        };
        if !readable {
            return Err(DosError::AccessDenied);
        }
        self.handle_type(handle)
    }

    pub(crate) fn read(&mut self, handle: u16, buffer: &mut [u8]) -> Result<usize, DosError> {
        if let OpenHandle::Device(device) = self.handle(handle)? {
            if !device.access.can_read() || !matches!(device.kind, Device::Null) {
                return Err(DosError::AccessDenied);
            }
            if !buffer.is_empty() {
                device.set_eof(true);
            }
            return Ok(0);
        }
        let mut open = self.file_mut(handle)?;
        if !open.access.can_read() {
            return Err(DosError::AccessDenied);
        }
        let count = open
            .file
            .read_at(buffer, u64::from(open.position))
            .map_err(|_| DosError::AccessDenied)?;
        // A host File handle can grow a mounted file beyond DOS's 32-bit offsets.
        open.position = open
            .position
            .checked_add(count as u32)
            .ok_or(DosError::AccessDenied)?;
        Ok(count)
    }

    pub(crate) fn write(&mut self, handle: u16, bytes: &[u8]) -> Result<usize, DosError> {
        if let OpenHandle::Device(device) = self.handle(handle)? {
            if !device.access.can_write() {
                return Err(DosError::AccessDenied);
            }
            // Even a zero-byte handle write clears EOF on this record and its duplicates.
            device.set_eof(false);
            let stream = match device.kind {
                Device::Console(stream) => stream,
                Device::Null => return Ok(bytes.len()),
                Device::Auxiliary | Device::Printer => return Err(DosError::AccessDenied),
            };
            if device.raw() {
                self.stdio.output(stream).extend(bytes);
                return Ok(bytes.len());
            }
            // Cooked console writes stop before Ctrl-Z and count source bytes,
            // including tabs expanded by the shared character-output path.
            let count = bytes
                .iter()
                .position(|&byte| byte == 0x1a)
                .unwrap_or(bytes.len());
            for &byte in &bytes[..count] {
                self.write_cooked_character(CharacterOutput::Console(stream), byte);
            }
            return Ok(count);
        }
        let mut open = self.file_mut(handle)?;
        if !open.access.can_write() {
            return Err(DosError::AccessDenied);
        }
        if bytes.is_empty() {
            // DOS AH=40h with CX=0 sets the file length at the current position.
            open.file
                .set_len(u64::from(open.position))
                .map_err(|_| DosError::AccessDenied)?;
            open.written = true;
            return Ok(0);
        }
        // DOS cursors and file sizes remain 32-bit even though VFS offsets are u64.
        let count = u32::try_from(bytes.len()).map_err(|_| DosError::AccessDenied)?;
        open.position
            .checked_add(count)
            .ok_or(DosError::AccessDenied)?;
        let count = open
            .file
            .write_at(bytes, u64::from(open.position))
            .map_err(|_| DosError::AccessDenied)?;
        open.position += count as u32;
        open.written = true;
        Ok(count)
    }

    pub(crate) fn seek(&mut self, handle: u16, origin: u8, offset: u32) -> Result<u32, DosError> {
        let open = self.handle(handle)?;
        if origin > 2 {
            return Err(DosError::InvalidFunction);
        }
        if matches!(open, OpenHandle::Device(_)) {
            return Ok(0);
        }
        let mut open = self.file_mut(handle)?;
        let base = match origin {
            0 => 0,
            1 => open.position,
            2 => u32::try_from(open.file.len()).map_err(|_| DosError::AccessDenied)?,
            _ => return Err(DosError::InvalidFunction),
        };
        // Absolute offsets use all 32 bits; relative offsets wrap as DOS ADD/ADC does.
        open.position = base.wrapping_add(offset);
        Ok(open.position)
    }

    pub(crate) fn close(&mut self, handle: u16) -> Result<(), DosError> {
        let open = self
            .handles
            .get_mut(usize::from(handle))
            .and_then(Option::take)
            .ok_or(DosError::InvalidHandle)?;
        self.commit_handle(open);
        Ok(())
    }

    pub(crate) fn commit(&mut self, handle: u16) -> Result<(), DosError> {
        let open = self.handle(handle)?.clone();
        self.commit_handle(open);
        Ok(())
    }

    fn commit_handle(&mut self, handle: OpenHandle) {
        let OpenHandle::File(open) = handle else {
            return;
        };
        // VFS writes are immediate; commit and close publish DOS archive metadata.
        let open = open.borrow();
        // The host may have replaced this path while the old file was still open.
        if open.written
            && self
                .drives
                .get(open.path.drive)
                .unwrap()
                .open(&open.path.relative)
                .is_ok_and(|file| file.same_file(&open.file))
        {
            *self.attributes.entry(open.path.clone()).or_default() |= ARCHIVE_ATTRIBUTE as u8;
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
            self.commit_handle(replaced);
        }
        Ok(())
    }

    pub(crate) fn device_information(&self, handle: u16) -> Result<u16, DosError> {
        const FILE_NOT_WRITTEN: u16 = 1 << 6;
        Ok(match self.handle(handle)? {
            OpenHandle::File(file) => {
                let file = file.borrow();
                u16::from(file.path.drive as u8 - b'A')
                    | if file.written { 0 } else { FILE_NOT_WRITTEN }
            }
            OpenHandle::Device(device) => device.information(),
        })
    }

    /// DOS status queries ask the device, not the buffered handle-read line or EOF bit.
    pub(crate) fn input_available(&self, handle: u16) -> Result<bool, DosError> {
        Ok(match self.handle(handle)? {
            OpenHandle::File(file) => {
                let file = file.borrow();
                u64::from(file.position) < file.file.len()
            }
            OpenHandle::Device(device) => match device.kind {
                Device::Console(_) => self.stdio.input.has_input(),
                Device::Null => true,
                Device::Auxiliary | Device::Printer => false,
            },
        })
    }

    pub(crate) fn output_ready(&self, handle: u16) -> Result<bool, DosError> {
        Ok(match self.handle(handle)? {
            // DOS reports files ready even when opened for reading only.
            OpenHandle::File(_) => true,
            OpenHandle::Device(device) => match device.kind {
                Device::Console(_) | Device::Null => true,
                // These standard device slots have no attached host streams yet.
                Device::Auxiliary | Device::Printer => false,
            },
        })
    }

    pub(crate) fn reset_handles(&mut self) {
        self.close_all();
        self.stdio.clear();
        self.search_directories.clear();
        for (slot, device) in self.handles.iter_mut().zip([
            Device::Console(OutputStream::Stdout),
            Device::Console(OutputStream::Stdout),
            Device::Console(OutputStream::Stderr),
            Device::Auxiliary,
            Device::Printer,
        ]) {
            *slot = Some(OpenHandle::Device(Rc::new(OpenDevice::new(
                device,
                AccessMode::ReadWrite,
            ))));
        }
    }

    pub(crate) fn close_all(&mut self) {
        for handle in std::mem::take(&mut self.handles).into_iter().flatten() {
            self.commit_handle(handle);
        }
    }

    pub(super) fn handle(&self, handle: u16) -> Result<&OpenHandle, DosError> {
        self.handles
            .get(usize::from(handle))
            .and_then(Option::as_ref)
            .ok_or(DosError::InvalidHandle)
    }

    fn file_mut(&self, handle: u16) -> Result<RefMut<'_, OpenFile>, DosError> {
        match self.handle(handle)? {
            OpenHandle::File(file) => Ok(file.borrow_mut()),
            _ => Err(DosError::AccessDenied),
        }
    }
}
