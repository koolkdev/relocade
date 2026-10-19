//! Directory operations and C:'s current directory.

use super::Files;
use crate::{error::DosError, path};
use std::io::ErrorKind;

impl Files {
    pub(crate) fn create_directory(&mut self, path: &[u8]) -> Result<(), DosError> {
        let directory =
            path::resolve(path, &self.current_directory).ok_or(DosError::PathNotFound)?;
        self.filesystem
            .create_dir(&directory)
            .map_err(|error| match error.kind() {
                ErrorKind::NotFound | ErrorKind::NotADirectory => DosError::PathNotFound,
                _ => DosError::AccessDenied,
            })
    }

    pub(crate) fn remove_directory(&mut self, path: &[u8]) -> Result<(), DosError> {
        let directory =
            path::resolve(path, &self.current_directory).ok_or(DosError::PathNotFound)?;
        if directory.is_empty() {
            return Err(DosError::AccessDenied);
        }
        if directory == self.current_directory {
            return Err(DosError::CurrentDirectory);
        }
        let metadata = self
            .filesystem
            .metadata(&directory)
            .map_err(|_| DosError::PathNotFound)?;
        if !metadata.is_dir() {
            return Err(DosError::AccessDenied);
        }
        self.filesystem
            .remove_dir(&directory)
            .map_err(|_| DosError::AccessDenied)?;
        self.attributes.remove(&directory);
        Ok(())
    }

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
}
