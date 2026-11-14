//! DOS directory metadata shared by open files, path queries and searches.

use super::{
    FileSystem, FileTimestamp, ARCHIVE_ATTRIBUTE, DIRECTORY_ATTRIBUTE, HIDDEN_ATTRIBUTE,
    READ_ONLY_ATTRIBUTE, SYSTEM_ATTRIBUTE,
};
use crate::{error::DosError, path::ResolvedPath};
use relocade_vfs::Metadata;

#[derive(Clone, Copy)]
pub(super) struct EntryMetadata {
    pub(super) attributes: u8,
    pub(super) modified: FileTimestamp,
}

impl FileSystem {
    pub(crate) fn attributes(&self, path: &[u8]) -> Result<u16, DosError> {
        let path = self.resolve(path)?;
        Ok(u16::from(
            self.entry_metadata(&path, &self.metadata(&path)?)
                .attributes,
        ))
    }

    pub(crate) fn set_attributes(&mut self, path: &[u8], attributes: u16) -> Result<(), DosError> {
        let path = self.resolve(path)?;
        let metadata = self.metadata(&path)?;
        let mutable = READ_ONLY_ATTRIBUTE | HIDDEN_ATTRIBUTE | SYSTEM_ATTRIBUTE | ARCHIVE_ATTRIBUTE;
        // The directory bit may be retained by a read-modify-write caller, but
        // cannot turn a file into a directory. Root has no directory entry.
        if path.relative.is_empty()
            || attributes & !(mutable | u16::from(DIRECTORY_ATTRIBUTE)) != 0
            || (attributes & u16::from(DIRECTORY_ATTRIBUTE) != 0 && !metadata.is_dir())
        {
            return Err(DosError::AccessDenied);
        }
        if metadata.is_file() {
            self.drives
                .get(path.drive)
                .unwrap()
                .open(&path.relative)
                .expect("existing file")
                .set_read_only(attributes & READ_ONLY_ATTRIBUTE != 0);
        }
        let mut entry = self.entry_metadata(&path, &metadata);
        entry.attributes = (attributes & mutable) as u8;
        self.entries.insert(path, entry);
        Ok(())
    }

    pub(super) fn entry_metadata(&self, path: &ResolvedPath, metadata: &Metadata) -> EntryMetadata {
        let mut entry = self.entries.get(path).copied().unwrap_or(EntryMetadata {
            attributes: if metadata.is_dir() {
                0
            } else {
                ARCHIVE_ATTRIBUTE as u8
            },
            modified: self.mount_timestamps[&path.drive],
        });
        entry.attributes = if metadata.is_dir() {
            entry.attributes | DIRECTORY_ATTRIBUTE
        } else {
            (entry.attributes & !(READ_ONLY_ATTRIBUTE as u8)) | u8::from(metadata.is_read_only())
        };
        entry
    }

    pub(super) fn timestamp(&self) -> FileTimestamp {
        FileTimestamp::from_datetime(self.clock.now())
    }
}
