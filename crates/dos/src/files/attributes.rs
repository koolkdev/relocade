//! DOS attributes shared by path queries and directory searches.

use super::{
    Files, ARCHIVE_ATTRIBUTE, DIRECTORY_ATTRIBUTE, HIDDEN_ATTRIBUTE, READ_ONLY_ATTRIBUTE,
    SYSTEM_ATTRIBUTE,
};
use crate::{error::DosError, path};
use relocade_vfs::Metadata;

impl Files {
    pub(crate) fn attributes(&self, path: &[u8]) -> Result<u16, DosError> {
        let path = path::resolve(path, &self.current_directory).ok_or(DosError::PathNotFound)?;
        Ok(u16::from(
            self.entry_attributes(&path, &self.metadata(&path)?),
        ))
    }

    pub(crate) fn set_attributes(&mut self, path: &[u8], attributes: u16) -> Result<(), DosError> {
        let path = path::resolve(path, &self.current_directory).ok_or(DosError::PathNotFound)?;
        let metadata = self.metadata(&path)?;
        let mutable = READ_ONLY_ATTRIBUTE | HIDDEN_ATTRIBUTE | SYSTEM_ATTRIBUTE | ARCHIVE_ATTRIBUTE;
        // The directory bit may be retained by a read-modify-write caller, but
        // cannot turn a file into a directory. Root has no directory entry.
        if path.is_empty()
            || attributes & !(mutable | u16::from(DIRECTORY_ATTRIBUTE)) != 0
            || (attributes & u16::from(DIRECTORY_ATTRIBUTE) != 0 && !metadata.is_dir())
        {
            return Err(DosError::AccessDenied);
        }
        if metadata.is_file() {
            self.filesystem
                .open(&path)
                .expect("existing file")
                .set_read_only(attributes & READ_ONLY_ATTRIBUTE != 0);
        }
        self.attributes.insert(path, (attributes & mutable) as u8);
        Ok(())
    }

    pub(super) fn metadata(&self, path: &str) -> Result<Metadata, DosError> {
        self.filesystem.metadata(path).map_err(|_| {
            if self.filesystem.is_dir(path::parent(path)) {
                DosError::FileNotFound
            } else {
                DosError::PathNotFound
            }
        })
    }

    pub(super) fn entry_attributes(&self, path: &str, metadata: &Metadata) -> u8 {
        let attributes = self.attributes.get(path).copied().unwrap_or_else(|| {
            if metadata.is_dir() {
                0
            } else {
                ARCHIVE_ATTRIBUTE as u8
            }
        });
        if metadata.is_dir() {
            attributes | DIRECTORY_ATTRIBUTE
        } else {
            (attributes & !(READ_ONLY_ATTRIBUTE as u8)) | u8::from(metadata.is_read_only())
        }
    }
}
