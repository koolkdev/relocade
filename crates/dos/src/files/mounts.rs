//! Drive mounts and the DOS paths that select them.

use super::Files;
use crate::{
    error::DosError,
    path::{self, ResolvedPath},
};
use relocade_vfs::{Drives, MemoryFileSystem};
use std::{collections::BTreeMap, io};

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
    pub(crate) fn mount(&mut self, letter: char, filesystem: MemoryFileSystem) -> io::Result<()> {
        self.drives.mount(letter, filesystem)?;
        let letter = letter.to_ascii_uppercase();
        self.current_directories.remove(&letter);
        self.attributes.retain(|path, _| path.drive != letter);
        // Keep DTA directory IDs stable; old searches must not enter the new mount.
        for directory in &mut self.search_directories {
            if directory
                .as_ref()
                .is_some_and(|directory| directory.path.drive == letter)
            {
                *directory = None;
            }
        }
        Ok(())
    }

    pub(crate) fn select_drive(&mut self, number: u8) -> u8 {
        if number < 26 {
            let letter = char::from(b'A' + number);
            if self.drives.get(letter).is_some() {
                self.current_drive = letter;
            }
        }
        // DOS 3+ reports at least five potentially valid drive letters.
        self.drives
            .letters()
            .next_back()
            .map_or(5, |letter| (letter as u8 - b'A' + 1).max(5))
    }

    pub(crate) fn current_drive(&self) -> u8 {
        self.current_drive as u8 - b'A'
    }

    fn path_context(&self, path: &[u8]) -> Result<(char, &str), DosError> {
        let drive = path::drive(path, self.current_drive).ok_or(DosError::PathNotFound)?;
        self.drives.get(drive).ok_or(DosError::PathNotFound)?;
        Ok((
            drive,
            self.current_directories
                .get(&drive)
                .map_or("", String::as_str),
        ))
    }

    pub(super) fn resolve(&self, path: &[u8]) -> Result<ResolvedPath, DosError> {
        let (drive, directory) = self.path_context(path)?;
        let relative = path::resolve(path, directory).ok_or(DosError::PathNotFound)?;
        Ok(ResolvedPath { drive, relative })
    }

    pub(super) fn file_path(&self, path: &[u8]) -> Result<ResolvedPath, DosError> {
        let (drive, directory) = self.path_context(path)?;
        let relative = path::file_path(path, directory).ok_or(DosError::PathNotFound)?;
        Ok(ResolvedPath { drive, relative })
    }

    // Host file helpers keep their existing root-relative default on C:.
    pub(super) fn host_path(&self, path: &str) -> Option<ResolvedPath> {
        let drive = path::drive(path.as_bytes(), 'C')?;
        self.drives.get(drive)?;
        let relative = path::file_path(path.as_bytes(), "")?;
        Some(ResolvedPath { drive, relative })
    }
}
