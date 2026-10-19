//! DOS path resolution and the current program's open file handles.

use crate::error::DosError;
use crate::MountError;
use std::io::ErrorKind;
use wasm86_vfs::{File, MemoryFileSystem};

// A DOS process starts with 20 handles, including its five standard devices.
const HANDLE_COUNT: usize = 20;
pub(crate) const MAX_PATH_BYTES: usize = 128; // Includes the terminating NUL.
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
    pub(crate) fn mount(
        &mut self,
        path: &str,
        contents: Vec<u8>,
        access: FileAccess,
    ) -> Result<(), MountError> {
        let path = normalize_path(path.as_bytes()).ok_or(MountError::InvalidPath)?;
        if contents.len() > u32::MAX as usize {
            return Err(MountError::FileTooLarge);
        }
        self.filesystem
            .create_dir_all(parent_path(&path))
            .map_err(|_| MountError::InvalidPath)?;
        let file = self
            .filesystem
            .replace_file(&path, contents)
            .map_err(|_| MountError::InvalidPath)?;
        file.set_read_only(access == FileAccess::ReadOnly);
        Ok(())
    }

    pub(crate) fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        self.filesystem.read(&normalize_path(path.as_bytes())?).ok()
    }

    pub(crate) fn open(&mut self, path: &[u8], mode: u8) -> Result<u16, DosError> {
        if mode & 0x0f > 2 || (mode >> 4) & 7 > 4 {
            return Err(DosError::InvalidAccess);
        }
        let path = normalize_path(path).ok_or(DosError::PathNotFound)?;
        let file = if mode & 3 == 0 {
            self.filesystem.open(&path)
        } else {
            self.filesystem.open_read_write(&path)
        }
        .map_err(|error| match error.kind() {
            ErrorKind::NotFound if self.filesystem.is_dir(parent_path(&path)) => {
                DosError::FileNotFound
            }
            ErrorKind::NotFound | ErrorKind::NotADirectory => DosError::PathNotFound,
            _ => DosError::AccessDenied,
        })?;
        self.open_file(file, mode)
    }

    pub(crate) fn create(&mut self, path: &[u8], attributes: u16) -> Result<u16, DosError> {
        // Direct file access only enforces read-only; the other file flags are accepted.
        if attributes
            & !(READ_ONLY_ATTRIBUTE | HIDDEN_ATTRIBUTE | SYSTEM_ATTRIBUTE | ARCHIVE_ATTRIBUTE)
            != 0
        {
            return Err(DosError::AccessDenied);
        }
        let path = normalize_path(path).ok_or(DosError::PathNotFound)?;
        if !self.filesystem.is_dir(parent_path(&path)) {
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

fn parent_path(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

// C: is the only drive and its current directory is the root. Mounted names use
// ASCII case-insensitive matching, with either slash accepted as a separator.
fn normalize_path(path: &[u8]) -> Option<String> {
    if path.is_empty() || path.len() >= MAX_PATH_BYTES || !path.is_ascii() {
        return None;
    }
    let path = std::str::from_utf8(path).ok()?;
    let path = if path.as_bytes().get(1) == Some(&b':') {
        if !path.as_bytes()[0].eq_ignore_ascii_case(&b'C') {
            return None;
        }
        &path[2..]
    } else {
        path
    };
    if matches!(path.rsplit(['\\', '/']).next()?, "" | "." | "..") {
        return None;
    }
    let mut components = Vec::new();
    for component in path.split(['\\', '/']) {
        match component {
            "" | "." => continue,
            ".." => {
                components.pop()?;
            }
            _ => {
                if component.bytes().any(|byte| {
                    byte < 0x20 || matches!(byte, b':' | b'"' | b'<' | b'>' | b'|' | b'*' | b'?')
                }) {
                    return None;
                }
                components.push(component);
            }
        }
    }
    if components.is_empty() {
        return None;
    }
    Some(components.join("/").to_ascii_uppercase())
}
