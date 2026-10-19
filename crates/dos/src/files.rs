//! DOS path resolution and the current program's open file handles.

use crate::error::DosError;
use crate::{path, MountError};
use std::{collections::BTreeMap, io::ErrorKind};

mod search;
mod short_name;
pub(crate) use search::FIND_DATA_BYTES;
use wasm86_vfs::{File, MemoryFileSystem};

// A DOS process starts with 20 handles, including its five standard devices.
const HANDLE_COUNT: usize = 20;
const READ_ONLY_ATTRIBUTE: u16 = 0x01;
const HIDDEN_ATTRIBUTE: u16 = 0x02;
const SYSTEM_ATTRIBUTE: u16 = 0x04;
const ARCHIVE_ATTRIBUTE: u16 = 0x20;

/// Whether a host-supplied file may be opened for writing.
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
    search_directories: Vec<String>,
    handles: [Option<OpenHandle>; HANDLE_COUNT],
}

enum OpenHandle {
    File(OpenFile),
    ConsoleInput,
    ConsoleOutput,
    Auxiliary,
    Printer,
}

struct OpenFile {
    file: File,
    position: u32,
    readable: bool,
    writable: bool,
    deny_read: bool,
    deny_write: bool,
    compatibility: bool,
}

impl Files {
    pub(crate) fn change_directory(&mut self, path: &[u8]) -> Result<(), DosError> {
        let directory =
            path::resolve(path, &self.current_directory).ok_or(DosError::PathNotFound)?;
        // AH=47h's caller supplies a 64-byte buffer, including the terminator.
        if directory.len() >= 64 || !self.filesystem.is_dir(&directory) {
            return Err(DosError::PathNotFound);
        }
        self.current_directory = directory;
        Ok(())
    }

    pub(crate) fn current_directory(&self, drive: u8) -> Result<&str, DosError> {
        // AH=47h uses 0 for the default drive and 1-based explicit drive numbers.
        match drive {
            0 | 3 => Ok(&self.current_directory),
            _ => Err(DosError::InvalidDrive),
        }
    }

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
        self.open_file(file, mode)
    }

    pub(crate) fn create(&mut self, path: &[u8], attributes: u16) -> Result<u16, DosError> {
        // DOS owns hidden/system/archive flags; VFS enforces read-only opens.
        if attributes
            & !(READ_ONLY_ATTRIBUTE | HIDDEN_ATTRIBUTE | SYSTEM_ATTRIBUTE | ARCHIVE_ATTRIBUTE)
            != 0
        {
            return Err(DosError::AccessDenied);
        }
        let path = path::file_path(path, &self.current_directory).ok_or(DosError::PathNotFound)?;
        if !self.filesystem.is_dir(path::parent(&path)) {
            return Err(DosError::PathNotFound);
        }
        let file = match self.filesystem.open_read_write(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                // Check capacity before publishing a new file. It cannot share
                // identity with an existing handle, so sharing checks will pass.
                self.vacant_handle()?;
                self.filesystem
                    .create_new(&path)
                    .map_err(|_| DosError::AccessDenied)?
            }
            Err(_) => return Err(DosError::AccessDenied),
        };
        // Existing files are truncated only after sharing and capacity checks.
        let handle = self.open_file(file.clone(), 2)?;
        file.set_len(0)
            .expect("truncating a writable file does not allocate");
        file.set_read_only(attributes & READ_ONLY_ATTRIBUTE != 0);
        self.attributes
            .insert(path, (attributes | ARCHIVE_ATTRIBUTE) as u8);
        Ok(handle)
    }

    fn open_file(&mut self, file: File, mode: u8) -> Result<u16, DosError> {
        let entry = OpenFile {
            file,
            position: 0,
            readable: mode & 3 != 1,
            writable: mode & 3 != 0,
            deny_read: matches!(mode & 0x70, 0x10 | 0x30),
            deny_write: matches!(mode & 0x70, 0x10 | 0x20),
            compatibility: mode & 0x70 == 0,
        };
        let read_only = entry.file.is_read_only();
        if self.handles.iter().flatten().any(|handle| {
            let OpenHandle::File(open) = handle else {
                return false;
            };
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
        self.handles[handle] = Some(OpenHandle::File(entry));
        Ok(handle as u16)
    }

    fn vacant_handle(&self) -> Result<usize, DosError> {
        self.handles
            .iter()
            .position(Option::is_none)
            .ok_or(DosError::TooManyOpenFiles)
    }

    pub(crate) fn read(&mut self, handle: u16, buffer: &mut [u8]) -> Result<usize, DosError> {
        let open = self.file_mut(handle)?;
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
        let open = self.file_mut(handle)?;
        if !open.writable {
            return Err(DosError::AccessDenied);
        }
        if bytes.is_empty() {
            // DOS AH=40h with CX=0 sets the file length at the current position.
            open.file
                .set_len(u64::from(open.position))
                .map_err(|_| DosError::AccessDenied)?;
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
        Ok(count)
    }

    pub(crate) fn seek(&mut self, handle: u16, origin: u8, offset: u32) -> Result<u32, DosError> {
        let open = self.file_mut(handle)?;
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
        self.handles
            .get_mut(usize::from(handle))
            .and_then(Option::take)
            .map(|_| ())
            .ok_or(DosError::InvalidHandle)
    }

    pub(crate) fn is_console_output(&self, handle: u16) -> bool {
        matches!(
            self.handles.get(usize::from(handle)),
            Some(Some(OpenHandle::ConsoleOutput))
        )
    }

    pub(crate) fn reset_handles(&mut self) {
        self.close_all();
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
        self.handles.fill_with(|| None);
    }

    fn file_mut(&mut self, handle: u16) -> Result<&mut OpenFile, DosError> {
        match self
            .handles
            .get_mut(usize::from(handle))
            .and_then(Option::as_mut)
        {
            Some(OpenHandle::File(file)) => Ok(file),
            Some(_) => Err(DosError::AccessDenied),
            None => Err(DosError::InvalidHandle),
        }
    }
}
