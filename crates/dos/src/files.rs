//! DOS filesystems, path operations and the current program's file state.

use crate::{
    error::DosError,
    path::{self, ResolvedPath},
};
use relocade_vfs::{Drives, MemoryFileSystem, Metadata};
use std::{collections::BTreeMap, io::ErrorKind};

mod attributes;
mod create;
mod directories;
mod handles;
mod mounts;
mod search;
mod short_name;
pub(crate) use create::CreateMode;
use handles::{OpenHandle, HANDLE_COUNT};
pub(crate) use search::FIND_DATA_BYTES;

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

pub(crate) struct Files {
    drives: Drives,
    current_drive: char,
    current_directories: BTreeMap<char, String>,
    attributes: BTreeMap<ResolvedPath, u8>,
    next_temporary_name: u32,
    search_directories: Vec<Option<search::SearchDirectory>>,
    handles: [Option<OpenHandle>; HANDLE_COUNT],
    output: Vec<u8>,
}

impl Default for Files {
    fn default() -> Self {
        let mut drives = Drives::new();
        drives.mount('C', MemoryFileSystem::new()).unwrap();
        Self {
            drives,
            current_drive: 'C',
            current_directories: BTreeMap::new(),
            attributes: BTreeMap::new(),
            next_temporary_name: 0,
            search_directories: Vec::new(),
            handles: Default::default(),
            output: Vec::new(),
        }
    }
}

impl Files {
    pub(crate) fn delete(&mut self, path: &[u8]) -> Result<(), DosError> {
        let path = self.file_path(path)?;
        let metadata = self.metadata(&path)?;
        if metadata.is_dir() || metadata.is_read_only() {
            return Err(DosError::AccessDenied);
        }
        let file = self
            .drives
            .get(path.drive)
            .unwrap()
            .open(&path.relative)
            .expect("existing file");
        if self.is_open(&file) {
            return Err(DosError::AccessDenied);
        }
        self.drives
            .get_mut(path.drive)
            .unwrap()
            .remove_file(&path.relative)
            .expect("existing file");
        self.attributes.remove(&path);
        Ok(())
    }

    pub(crate) fn rename(&mut self, from: &[u8], to: &[u8]) -> Result<(), DosError> {
        let from_drive = path::drive(from, self.current_drive).ok_or(DosError::PathNotFound)?;
        let to_drive = path::drive(to, self.current_drive).ok_or(DosError::PathNotFound)?;
        if from_drive != to_drive {
            return Err(DosError::NotSameDevice);
        }
        let from = self.file_path(from)?;
        let to = self.file_path(to)?;
        if self.metadata(&from)?.is_dir() {
            return Err(DosError::AccessDenied);
        }
        let filesystem = self.drives.get(from.drive).unwrap();
        let file = filesystem.open(&from.relative).expect("existing file");
        if self.is_open(&file) {
            return Err(DosError::AccessDenied);
        }
        if !filesystem.is_dir(path::parent(&to.relative)) {
            return Err(DosError::PathNotFound);
        }
        // DOS never replaces an existing destination, including the source itself.
        if filesystem.metadata(&to.relative).is_ok() {
            return Err(DosError::AccessDenied);
        }
        self.drives
            .get_mut(from.drive)
            .unwrap()
            .rename_file(&from.relative, &to.relative)
            .expect("validated file paths");
        if let Some(attributes) = self.attributes.remove(&from) {
            self.attributes.insert(to, attributes);
        }
        Ok(())
    }

    pub(crate) fn open(&mut self, path: &[u8], mode: u8) -> Result<u16, DosError> {
        if mode & 0x0f > 2 || (mode >> 4) & 7 > 4 {
            return Err(DosError::InvalidAccess);
        }
        let path = self.file_path(path)?;
        let filesystem = self.drives.get(path.drive).unwrap();
        let file = if mode & 3 == 0 {
            filesystem.open(&path.relative)
        } else {
            filesystem.open_read_write(&path.relative)
        }
        .map_err(|error| match error.kind() {
            ErrorKind::NotFound if filesystem.is_dir(path::parent(&path.relative)) => {
                DosError::FileNotFound
            }
            ErrorKind::NotFound | ErrorKind::NotADirectory => DosError::PathNotFound,
            _ => DosError::AccessDenied,
        })?;
        self.open_file(file, path, mode)
    }

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
