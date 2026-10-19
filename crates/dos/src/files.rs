//! DOS path resolution and the current program's open file handles.

use crate::error::DosError;
use crate::filesystem::{File, FileSystem};
use crate::{FileAccess, MountError};
use std::cell::RefCell;
use std::rc::Rc;

// A DOS process starts with 20 handles, including its five standard devices.
const HANDLE_COUNT: usize = 20;
pub(crate) const MAX_PATH_BYTES: usize = 128; // Includes the terminating NUL.

#[derive(Default)]
pub(crate) struct Files {
    filesystem: FileSystem,
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
    file: Rc<RefCell<File>>,
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
        self.filesystem.mount(path, contents, access);
        Ok(())
    }

    pub(crate) fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        self.filesystem.read_file(&normalize_path(path.as_bytes())?)
    }

    pub(crate) fn open(&mut self, path: &[u8], mode: u8) -> Result<u16, DosError> {
        if mode & 0x0f > 2 || (mode >> 4) & 7 > 4 {
            return Err(DosError::InvalidAccess);
        }
        let path = normalize_path(path).ok_or(DosError::PathNotFound)?;
        let file = self.filesystem.file(&path).ok_or_else(|| {
            if self.filesystem.parent_exists(&path) {
                DosError::FileNotFound
            } else {
                DosError::PathNotFound
            }
        })?;
        if mode & 3 != 0 && file.borrow().access == FileAccess::ReadOnly {
            return Err(DosError::AccessDenied);
        }
        self.open_file(Rc::clone(file), mode)
    }

    fn open_file(&mut self, file: Rc<RefCell<File>>, mode: u8) -> Result<u16, DosError> {
        let entry = OpenFile {
            file,
            position: 0,
            readable: mode & 3 != 1,
            writable: mode & 3 != 0,
            deny_read: matches!(mode & 0x70, 0x10 | 0x30),
            deny_write: matches!(mode & 0x70, 0x10 | 0x20),
            compatibility: mode & 0x70 == 0,
        };
        let read_only = entry.file.borrow().access == FileAccess::ReadOnly;
        if self.handles.iter().flatten().any(|handle| {
            let OpenHandle::File(open) = handle else {
                return false;
            };
            Rc::ptr_eq(&open.file, &entry.file)
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
        let (handle, slot) = self
            .handles
            .iter_mut()
            .enumerate()
            .find(|(_, entry)| entry.is_none())
            .ok_or(DosError::TooManyOpenFiles)?;
        *slot = Some(OpenHandle::File(entry));
        Ok(handle as u16)
    }

    pub(crate) fn read(&mut self, handle: u16, buffer: &mut [u8]) -> Result<usize, DosError> {
        let open = self.file_mut(handle)?;
        if !open.readable {
            return Err(DosError::AccessDenied);
        }
        let count = open.file.borrow().read_at(open.position, buffer);
        open.position += count as u32;
        Ok(count)
    }

    pub(crate) fn write(&mut self, handle: u16, bytes: &[u8]) -> Result<usize, DosError> {
        let open = self.file_mut(handle)?;
        if !open.writable {
            return Err(DosError::AccessDenied);
        }
        let mut storage = open.file.borrow_mut();
        if bytes.is_empty() {
            // DOS AH=40h with CX=0 sets the file length at the current position.
            storage
                .set_len(open.position)
                .map_err(|_| DosError::AccessDenied)?;
            return Ok(0);
        }
        let count = storage
            .write_at(open.position, bytes)
            .map_err(|_| DosError::AccessDenied)?;
        open.position += count as u32;
        Ok(count)
    }

    pub(crate) fn seek(&mut self, handle: u16, origin: u8, offset: u32) -> Result<u32, DosError> {
        let open = self.file_mut(handle)?;
        let base = match origin {
            0 => 0,
            1 => open.position,
            2 => open.file.borrow().len(),
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
