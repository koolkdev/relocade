//! Persistent DOS filesystems and the open resources of one process.

use crate::{
    error::DosError,
    path::{self, ResolvedPath},
};
use relocade_vfs::{Drives, MemoryFileSystem, Metadata};
use std::{collections::BTreeMap, io::ErrorKind};

mod attributes;
mod character;
mod create;
mod devices;
mod directories;
mod handles;
mod mounts;
mod search;
mod short_name;
pub(crate) use character::CharacterInput;
pub(crate) use create::CreateMode;
pub(crate) use devices::Device;
pub(crate) use handles::HandleType;
use handles::{OpenHandle, HANDLE_COUNT};
pub(crate) use search::FIND_DATA_BYTES;

const READ_ONLY_ATTRIBUTE: u16 = 0x01;
const HIDDEN_ATTRIBUTE: u16 = 0x02;
const SYSTEM_ATTRIBUTE: u16 = 0x04;
const ARCHIVE_ATTRIBUTE: u16 = 0x20;
const DIRECTORY_ATTRIBUTE: u8 = 0x10;
const COMPATIBILITY_SHARING: u8 = 0;

/// Initial read-only attribute of a mounted file; DOS AH=43h can change it later.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileAccess {
    ReadOnly,
    ReadWrite,
}

/// Access granted to an open file or character-device handle.
#[derive(Clone, Copy)]
enum AccessMode {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

impl AccessMode {
    fn can_read(self) -> bool {
        !matches!(self, Self::WriteOnly)
    }

    fn can_write(self) -> bool {
        !matches!(self, Self::ReadOnly)
    }
}

pub(crate) struct FileSystem {
    drives: Drives,
    current_drive: char,
    current_directories: BTreeMap<char, String>,
    attributes: BTreeMap<ResolvedPath, u8>,
    next_temporary_name: u32,
}

pub(crate) struct Files {
    handles: [Option<OpenHandle>; HANDLE_COUNT],
    search_directories: Vec<Option<search::SearchDirectory>>,
}

impl Default for FileSystem {
    fn default() -> Self {
        let mut drives = Drives::new();
        drives.mount('C', MemoryFileSystem::new()).unwrap();
        Self {
            drives,
            current_drive: 'C',
            current_directories: BTreeMap::new(),
            attributes: BTreeMap::new(),
            next_temporary_name: 0,
        }
    }
}

impl Files {
    pub(crate) fn delete(&self, filesystem: &mut FileSystem, path: &[u8]) -> Result<(), DosError> {
        let path = filesystem.file_path(path)?;
        let metadata = filesystem.metadata(&path)?;
        if metadata.is_dir() || metadata.is_read_only() {
            return Err(DosError::AccessDenied);
        }
        let file = filesystem
            .drives
            .get(path.drive)
            .unwrap()
            .open(&path.relative)
            .expect("existing file");
        if self.is_open(&file) {
            return Err(DosError::AccessDenied);
        }
        filesystem
            .drives
            .get_mut(path.drive)
            .unwrap()
            .remove_file(&path.relative)
            .expect("existing file");
        filesystem.attributes.remove(&path);
        Ok(())
    }

    pub(crate) fn rename(
        &self,
        filesystem: &mut FileSystem,
        from: &[u8],
        to: &[u8],
    ) -> Result<(), DosError> {
        let from_drive =
            path::drive(from, filesystem.current_drive).ok_or(DosError::PathNotFound)?;
        let to_drive = path::drive(to, filesystem.current_drive).ok_or(DosError::PathNotFound)?;
        if from_drive != to_drive {
            return Err(DosError::NotSameDevice);
        }
        let from = filesystem.file_path(from)?;
        let to = filesystem.file_path(to)?;
        if filesystem.metadata(&from)?.is_dir() {
            return Err(DosError::AccessDenied);
        }
        let drive = filesystem.drives.get(from.drive).unwrap();
        let file = drive.open(&from.relative).expect("existing file");
        if self.is_open(&file) {
            return Err(DosError::AccessDenied);
        }
        if !drive.is_dir(path::parent(&to.relative)) {
            return Err(DosError::PathNotFound);
        }
        // DOS never replaces an existing destination, including the source itself.
        if drive.metadata(&to.relative).is_ok() {
            return Err(DosError::AccessDenied);
        }
        filesystem
            .drives
            .get_mut(from.drive)
            .unwrap()
            .rename_file(&from.relative, &to.relative)
            .expect("validated file paths");
        if let Some(attributes) = filesystem.attributes.remove(&from) {
            filesystem.attributes.insert(to, attributes);
        }
        Ok(())
    }

    pub(crate) fn open(
        &mut self,
        filesystem: &FileSystem,
        path: &[u8],
        mode: u8,
    ) -> Result<u16, DosError> {
        let access = match mode & 0x0f {
            0 => AccessMode::ReadOnly,
            1 => AccessMode::WriteOnly,
            2 => AccessMode::ReadWrite,
            _ => return Err(DosError::InvalidAccess),
        };
        let sharing = mode & 0x70;
        if sharing > 0x40 {
            return Err(DosError::InvalidAccess);
        }
        if let Some(handle) = self.open_device(filesystem, path, access)? {
            return Ok(handle);
        }
        let path = filesystem.file_path(path)?;
        let drive = filesystem.drives.get(path.drive).unwrap();
        let file = if access.can_write() {
            drive.open_read_write(&path.relative)
        } else {
            drive.open(&path.relative)
        }
        .map_err(|error| match error.kind() {
            ErrorKind::NotFound if drive.is_dir(path::parent(&path.relative)) => {
                DosError::FileNotFound
            }
            ErrorKind::NotFound | ErrorKind::NotADirectory => DosError::PathNotFound,
            _ => DosError::AccessDenied,
        })?;
        self.open_file(file, path, access, sharing)
    }
}

impl FileSystem {
    fn metadata(&self, path: &ResolvedPath) -> Result<Metadata, DosError> {
        let filesystem = self.drives.get(path.drive).unwrap();
        filesystem.metadata(&path.relative).map_err(|_| {
            if filesystem.is_dir(path::parent(&path.relative)) {
                DosError::FileNotFound
            } else {
                DosError::PathNotFound
            }
        })
    }
}
