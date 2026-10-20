//! Directory operations and per-drive current directories.

use super::Files;
use crate::error::DosError;
use std::io::ErrorKind;

impl Files {
    pub(crate) fn create_directory(&mut self, path: &[u8]) -> Result<(), DosError> {
        let directory = self.resolve(path)?;
        self.drives
            .get_mut(directory.drive)
            .unwrap()
            .create_dir(&directory.relative)
            .map_err(|error| match error.kind() {
                ErrorKind::NotFound | ErrorKind::NotADirectory => DosError::PathNotFound,
                _ => DosError::AccessDenied,
            })
    }

    pub(crate) fn remove_directory(&mut self, path: &[u8]) -> Result<(), DosError> {
        let directory = self.resolve(path)?;
        if directory.relative.is_empty() {
            return Err(DosError::AccessDenied);
        }
        if self.current_directories.get(&directory.drive) == Some(&directory.relative) {
            return Err(DosError::CurrentDirectory);
        }
        let filesystem = self.drives.get_mut(directory.drive).unwrap();
        let metadata = filesystem
            .metadata(&directory.relative)
            .map_err(|_| DosError::PathNotFound)?;
        if !metadata.is_dir() {
            return Err(DosError::AccessDenied);
        }
        filesystem
            .remove_dir(&directory.relative)
            .map_err(|_| DosError::AccessDenied)?;
        self.attributes.remove(&directory);
        Ok(())
    }

    pub(crate) fn change_directory(&mut self, path: &[u8]) -> Result<(), DosError> {
        let directory = self.resolve(path)?;
        // AH=47h's caller supplies a 64-byte buffer, including the terminator.
        if directory.relative.len() >= 64
            || !self
                .drives
                .get(directory.drive)
                .unwrap()
                .is_dir(&directory.relative)
        {
            return Err(DosError::PathNotFound);
        }
        self.current_directories
            .insert(directory.drive, directory.relative);
        Ok(())
    }

    pub(crate) fn current_directory(&self, drive: u8) -> Result<&str, DosError> {
        // AH=47h uses 0 for the default drive and 1-based explicit drive numbers.
        let letter = match drive {
            0 => self.current_drive,
            1..=26 => char::from(b'A' + drive - 1),
            _ => return Err(DosError::InvalidDrive),
        };
        self.drives.get(letter).ok_or(DosError::InvalidDrive)?;
        Ok(self
            .current_directories
            .get(&letter)
            .map_or("", String::as_str))
    }
}
