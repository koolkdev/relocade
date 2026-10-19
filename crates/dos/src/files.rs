//! DOS path resolution and the current program's open file handles.

use crate::error::DosError;
use crate::{path, MountError};
use std::{
    cell::{RefCell, RefMut},
    collections::BTreeMap,
    io::ErrorKind,
    rc::Rc,
};

mod attributes;
mod create;
mod directories;
mod search;
mod short_name;
pub(crate) use create::CreateMode;
use relocade_vfs::{File, MemoryFileSystem, Metadata};
pub(crate) use search::FIND_DATA_BYTES;

// A DOS process starts with 20 handles, including its five standard devices.
const HANDLE_COUNT: usize = 20;
const READ_ONLY_ATTRIBUTE: u16 = 0x01;
const HIDDEN_ATTRIBUTE: u16 = 0x02;
const SYSTEM_ATTRIBUTE: u16 = 0x04;
const ARCHIVE_ATTRIBUTE: u16 = 0x20;
const DIRECTORY_ATTRIBUTE: u8 = 0x10;

/// Initial read-only attribute of a mounted file; DOS AH=43h can change it later.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileAccess {
    ReadOnly,
    ReadWrite,
}

#[derive(Default)]
pub(crate) struct Files {
    filesystem: MemoryFileSystem,
    current_directory: String,
    attributes: BTreeMap<String, u8>,
    next_temporary_name: u32,
    search_directories: Vec<search::SearchDirectory>,
    handles: [Option<OpenHandle>; HANDLE_COUNT],
    output: Vec<u8>,
}

#[derive(Clone)]
enum OpenHandle {
    // Separate opens have separate cursors; duplicated handles share this record.
    File(Rc<RefCell<OpenFile>>),
    ConsoleInput,
    ConsoleOutput,
    Auxiliary,
    Printer,
}

struct OpenFile {
    file: File,
    path: String,
    position: u32,
    readable: bool,
    writable: bool,
    deny_read: bool,
    deny_write: bool,
    compatibility: bool,
    written: bool,
}

impl Files {
    pub(crate) fn mount(
        &mut self,
        path: &str,
        contents: Vec<u8>,
        access: FileAccess,
    ) -> Result<(), MountError> {
        let path = path::file_path(path.as_bytes(), "").ok_or(MountError::InvalidPath)?;
        if contents.len() > u32::MAX as usize {
            return Err(MountError::FileTooLarge);
        }
        self.filesystem
            .create_dir_all(path::parent(&path))
            .map_err(|_| MountError::InvalidPath)?;
        let file = self
            .filesystem
            .replace_file(&path, contents)
            .map_err(|_| MountError::InvalidPath)?;
        file.set_read_only(access == FileAccess::ReadOnly);
        self.attributes.insert(path, ARCHIVE_ATTRIBUTE as u8);
        Ok(())
    }

    pub(crate) fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        self.filesystem
            .read(&path::file_path(path.as_bytes(), "")?)
            .ok()
    }

    pub(crate) fn delete(&mut self, path: &[u8]) -> Result<(), DosError> {
        let path = path::file_path(path, &self.current_directory).ok_or(DosError::PathNotFound)?;
        let metadata = self.metadata(&path)?;
        if metadata.is_dir() || metadata.is_read_only() {
            return Err(DosError::AccessDenied);
        }
        let file = self.filesystem.open(&path).expect("existing file");
        if self.is_open(&file) {
            return Err(DosError::AccessDenied);
        }
        self.filesystem.remove_file(&path).expect("existing file");
        self.attributes.remove(&path);
        Ok(())
    }

    pub(crate) fn rename(&mut self, from: &[u8], to: &[u8]) -> Result<(), DosError> {
        let from_drive = path::drive(from).ok_or(DosError::PathNotFound)?;
        let to_drive = path::drive(to).ok_or(DosError::PathNotFound)?;
        if from_drive != to_drive {
            return Err(DosError::NotSameDevice);
        }
        let from = path::file_path(from, &self.current_directory).ok_or(DosError::PathNotFound)?;
        let to = path::file_path(to, &self.current_directory).ok_or(DosError::PathNotFound)?;
        if self.metadata(&from)?.is_dir() {
            return Err(DosError::AccessDenied);
        }
        let file = self.filesystem.open(&from).expect("existing file");
        if self.is_open(&file) {
            return Err(DosError::AccessDenied);
        }
        if !self.filesystem.is_dir(path::parent(&to)) {
            return Err(DosError::PathNotFound);
        }
        // DOS never replaces an existing destination, including the source itself.
        if self.filesystem.metadata(&to).is_ok() {
            return Err(DosError::AccessDenied);
        }
        self.filesystem
            .rename_file(&from, &to)
            .expect("validated file paths");
        if let Some(attributes) = self.attributes.remove(&from) {
            self.attributes.insert(to, attributes);
        }
        Ok(())
    }

    fn is_open(&self, file: &File) -> bool {
        // DOS sharing keeps an open file's name alive until its last handle closes.
        self.handles.iter().flatten().any(
            |handle| matches!(handle, OpenHandle::File(open) if open.borrow().file.same_file(file)),
        )
    }

    pub(crate) fn open(&mut self, path: &[u8], mode: u8) -> Result<u16, DosError> {
        if mode & 0x0f > 2 || (mode >> 4) & 7 > 4 {
            return Err(DosError::InvalidAccess);
        }
        let path = path::file_path(path, &self.current_directory).ok_or(DosError::PathNotFound)?;
        let file = if mode & 3 == 0 {
            self.filesystem.open(&path)
        } else {
            self.filesystem.open_read_write(&path)
        }
        .map_err(|error| match error.kind() {
            ErrorKind::NotFound if self.filesystem.is_dir(path::parent(&path)) => {
                DosError::FileNotFound
            }
            ErrorKind::NotFound | ErrorKind::NotADirectory => DosError::PathNotFound,
            _ => DosError::AccessDenied,
        })?;
        self.open_file(file, path, mode)
    }

    fn open_file(&mut self, file: File, path: String, mode: u8) -> Result<u16, DosError> {
        let entry = OpenFile {
            file,
            path,
            position: 0,
            readable: mode & 3 != 1,
            writable: mode & 3 != 0,
            deny_read: matches!(mode & 0x70, 0x10 | 0x30),
            deny_write: matches!(mode & 0x70, 0x10 | 0x20),
            compatibility: mode & 0x70 == 0,
            written: false,
        };
        let read_only = entry.file.is_read_only();
        if self.handles.iter().flatten().any(|handle| {
            let OpenHandle::File(open) = handle else {
                return false;
            };
            let open = open.borrow();
            open.file.same_file(&entry.file)
                && ((entry.readable && open.deny_read)
                    || (entry.writable && open.deny_write)
                    // New deny-read/write modes exempt this process's existing accesses.
                    || (entry.deny_read && entry.deny_write)
                    // Compatibility and explicit sharing may mix only for read-only files.
                    || (open.compatibility != entry.compatibility
                        && !(read_only && !open.writable && !entry.writable)))
        }) {
            return Err(DosError::AccessDenied);
        }
        let handle = self.vacant_handle()?;
        self.handles[handle] = Some(OpenHandle::File(Rc::new(RefCell::new(entry))));
        Ok(handle as u16)
    }

    fn vacant_handle(&self) -> Result<usize, DosError> {
        self.handles
            .iter()
            .position(Option::is_none)
            .ok_or(DosError::TooManyOpenFiles)
    }

    pub(crate) fn read(&mut self, handle: u16, buffer: &mut [u8]) -> Result<usize, DosError> {
        let mut open = self.file_mut(handle)?;
        if !open.readable {
            return Err(DosError::AccessDenied);
        }
        let count = open
            .file
            .read_at(buffer, u64::from(open.position))
            .map_err(|_| DosError::AccessDenied)?;
        open.position += count as u32;
        Ok(count)
    }

    pub(crate) fn write(&mut self, handle: u16, bytes: &[u8]) -> Result<usize, DosError> {
        if matches!(self.handle(handle)?, OpenHandle::ConsoleOutput) {
            self.output.extend_from_slice(bytes);
            return Ok(bytes.len());
        }
        let mut open = self.file_mut(handle)?;
        if !open.writable {
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
        let mut open = self.file_mut(handle)?;
        let base = match origin {
            0 => 0,
            1 => open.position,
            2 => open.file.len() as u32,
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
                .filesystem
                .open(&open.path)
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
        const DRIVE_C: u16 = 2;
        const FILE_NOT_WRITTEN: u16 = 1 << 6;
        // Bit 7 identifies a device handle; DH includes the character-driver attribute.
        const CHARACTER_DEVICE: u16 = (1 << 15) | (1 << 7);
        const INPUT_NOT_EOF: u16 = 1 << 6;
        const STANDARD_INPUT: u16 = 1;
        const STANDARD_OUTPUT: u16 = 2;
        Ok(match self.handle(handle)? {
            OpenHandle::File(file) => {
                DRIVE_C
                    | if file.borrow().written {
                        0
                    } else {
                        FILE_NOT_WRITTEN
                    }
            }
            // Console handles describe CON, regardless of their access direction.
            OpenHandle::ConsoleInput | OpenHandle::ConsoleOutput => {
                CHARACTER_DEVICE | INPUT_NOT_EOF | STANDARD_INPUT | STANDARD_OUTPUT
            }
            OpenHandle::Auxiliary | OpenHandle::Printer => CHARACTER_DEVICE | INPUT_NOT_EOF,
        })
    }

    pub(crate) fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    pub(crate) fn reset_handles(&mut self) {
        self.close_all();
        self.output.clear();
        self.search_directories.clear();
        for (slot, device) in self.handles.iter_mut().zip([
            OpenHandle::ConsoleInput,
            OpenHandle::ConsoleOutput,
            OpenHandle::ConsoleOutput,
            OpenHandle::Auxiliary,
            OpenHandle::Printer,
        ]) {
            *slot = Some(device);
        }
    }

    pub(crate) fn close_all(&mut self) {
        for handle in std::mem::take(&mut self.handles).into_iter().flatten() {
            self.commit_handle(handle);
        }
    }

    fn metadata(&self, path: &str) -> Result<Metadata, DosError> {
        self.filesystem.metadata(path).map_err(|_| {
            if self.filesystem.is_dir(path::parent(path)) {
                DosError::FileNotFound
            } else {
                DosError::PathNotFound
            }
        })
    }

    fn handle(&self, handle: u16) -> Result<&OpenHandle, DosError> {
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
