//! Read-only mounted files and the current program's open handles.

use crate::error::DosError;
use crate::MountError;
use std::collections::BTreeMap;
use std::rc::Rc;

// A DOS process starts with 20 handles, including its five standard devices.
const HANDLE_COUNT: usize = 20;
pub(crate) const MAX_PATH_BYTES: usize = 128; // Includes the terminating NUL.

#[derive(Default)]
pub(crate) struct Files {
    mounted: BTreeMap<String, Rc<[u8]>>,
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
    contents: Rc<[u8]>,
    position: u32,
    deny_read: bool,
}

impl Files {
    pub(crate) fn mount(&mut self, path: &str, contents: Vec<u8>) -> Result<(), MountError> {
        let path = normalize_path(path.as_bytes()).ok_or(MountError::InvalidPath)?;
        if contents.len() > u32::MAX as usize {
            return Err(MountError::FileTooLarge);
        }
        self.mounted.insert(path, contents.into());
        Ok(())
    }

    pub(crate) fn open(&mut self, path: &[u8], mode: u8) -> Result<u16, DosError> {
        if mode & 0x0f > 2 || (mode >> 4) & 7 > 4 {
            return Err(DosError::InvalidAccess);
        }
        if mode & 0x0f != 0 {
            return Err(DosError::AccessDenied);
        }
        let path = normalize_path(path).ok_or(DosError::PathNotFound)?;
        let contents = self.mounted.get(&path).ok_or_else(|| {
            if let Some((parent, _)) = path.rsplit_once('\\') {
                let prefix = format!("{parent}\\");
                if !self.mounted.keys().any(|name| name.starts_with(&prefix)) {
                    return DosError::PathNotFound;
                }
            }
            DosError::FileNotFound
        })?;
        // Mounted files cannot be written; only denial of other reads matters.
        let deny_read = matches!(mode & 0x70, 0x10 | 0x30);
        if self.handles.iter().flatten().any(|handle| {
            matches!(handle, OpenHandle::File(file)
                if Rc::ptr_eq(&file.contents, contents) && (deny_read || file.deny_read))
        }) {
            return Err(DosError::AccessDenied);
        }
        let (handle, slot) = self
            .handles
            .iter_mut()
            .enumerate()
            .find(|(_, entry)| entry.is_none())
            .ok_or(DosError::TooManyOpenFiles)?;
        *slot = Some(OpenHandle::File(OpenFile {
            contents: Rc::clone(contents),
            position: 0,
            deny_read,
        }));
        Ok(handle as u16)
    }

    pub(crate) fn read(&mut self, handle: u16, buffer: &mut [u8]) -> Result<usize, DosError> {
        let file = self.file_mut(handle)?;
        let remaining = file
            .contents
            .get(file.position as usize..)
            .unwrap_or_default();
        let count = buffer.len().min(remaining.len());
        buffer[..count].copy_from_slice(&remaining[..count]);
        file.position += count as u32;
        Ok(count)
    }

    pub(crate) fn seek(&mut self, handle: u16, origin: u8, offset: u32) -> Result<u32, DosError> {
        let file = self.file_mut(handle)?;
        let base = match origin {
            0 => 0,
            1 => file.position,
            2 => file.contents.len() as u32,
            _ => return Err(DosError::InvalidFunction),
        };
        // Absolute offsets use all 32 bits; relative offsets wrap as DOS ADD/ADC does.
        file.position = base.wrapping_add(offset);
        Ok(file.position)
    }

    pub(crate) fn close(&mut self, handle: u16) -> Result<(), DosError> {
        self.handles
            .get_mut(usize::from(handle))
            .and_then(Option::take)
            .map(|_| ())
            .ok_or(DosError::InvalidHandle)
    }

    pub(crate) fn is_open(&self, handle: u16) -> bool {
        self.handles
            .get(usize::from(handle))
            .is_some_and(Option::is_some)
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
    Some(components.join("\\").to_ascii_uppercase())
}
