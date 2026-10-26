//! Drive mounts, host file access and the DOS paths that select them.

use super::{FileAccess, FileSystem, ARCHIVE_ATTRIBUTE};
use crate::{
    error::DosError,
    path::{self, ResolvedPath},
    MountError,
};
use relocade_vfs::MemoryFileSystem;
use std::io;

impl FileSystem {
    pub(crate) fn mount(&mut self, letter: char, filesystem: MemoryFileSystem) -> io::Result<()> {
        self.drives.mount(letter, filesystem)?;
        let letter = letter.to_ascii_uppercase();
        self.current_directories.remove(&letter);
        self.attributes.retain(|path, _| path.drive != letter);
        Ok(())
    }

    pub(crate) fn mount_file(
        &mut self,
        path: &str,
        contents: Vec<u8>,
        access: FileAccess,
    ) -> Result<(), MountError> {
        let path = self.host_path(path).ok_or(MountError::InvalidPath)?;
        if contents.len() > u32::MAX as usize {
            return Err(MountError::FileTooLarge);
        }
        let filesystem = self.drives.get_mut(path.drive).expect("mounted drive");
        filesystem
            .create_dir_all(path::parent(&path.relative))
            .map_err(|_| MountError::InvalidPath)?;
        let file = filesystem
            .replace_file(&path.relative, contents)
            .map_err(|_| MountError::InvalidPath)?;
        file.set_read_only(access == FileAccess::ReadOnly);
        self.attributes.insert(path, ARCHIVE_ATTRIBUTE as u8);
        Ok(())
    }

    pub(crate) fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        let path = self.host_path(path)?;
        self.drives.get(path.drive)?.read(&path.relative).ok()
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
    fn host_path(&self, path: &str) -> Option<ResolvedPath> {
        let drive = path::drive(path.as_bytes(), 'C')?;
        self.drives.get(drive)?;
        let relative = path::file_path(path.as_bytes(), "")?;
        Some(ResolvedPath { drive, relative })
    }
}
