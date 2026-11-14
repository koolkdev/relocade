//! An open file's shared cursor, access rights and pending metadata.

use super::{
    AccessMode, EntryMetadata, FileSystem, FileTimestamp, ARCHIVE_ATTRIBUTE, COMPATIBILITY_SHARING,
};
use crate::{error::DosError, path::ResolvedPath};
use relocade_vfs::File;
use std::{cell::RefCell, rc::Rc};

/// Duplicated DOS handles and in-progress operations retain the same record.
#[derive(Clone)]
pub(crate) struct FileHandle(Rc<RefCell<State>>);

pub(super) enum SeekOrigin {
    Start,
    Current,
    End,
}

struct State {
    file: File,
    path: ResolvedPath,
    position: u32,
    access: AccessMode,
    deny_read: bool,
    deny_write: bool,
    compatibility: bool,
    written: bool,
    modified: FileTimestamp,
    explicit_timestamp: bool,
}

impl FileHandle {
    pub(super) fn new(
        file: File,
        path: ResolvedPath,
        access: AccessMode,
        sharing: u8,
        modified: FileTimestamp,
    ) -> Result<Self, DosError> {
        if file.len() > u64::from(u32::MAX) {
            return Err(DosError::AccessDenied);
        }
        Ok(Self(Rc::new(RefCell::new(State {
            file,
            path,
            position: 0,
            access,
            deny_read: matches!(sharing, 0x10 | 0x30),
            deny_write: matches!(sharing, 0x10 | 0x20),
            compatibility: sharing == COMPATIBILITY_SHARING,
            written: false,
            modified,
            explicit_timestamp: false,
        }))))
    }

    pub(super) fn same_file(&self, file: &File) -> bool {
        self.0.borrow().file.same_file(file)
    }

    pub(super) fn conflicts_with(&self, incoming: &Self) -> bool {
        let existing = self.0.borrow();
        let incoming = incoming.0.borrow();
        existing.file.same_file(&incoming.file)
            && ((incoming.access.can_read() && existing.deny_read)
                || (incoming.access.can_write() && existing.deny_write)
                // New deny-read/write modes exempt this process's existing accesses.
                || (incoming.deny_read && incoming.deny_write)
                // Compatibility and explicit sharing may mix only for read-only files.
                || (existing.compatibility != incoming.compatibility
                    && !(incoming.file.is_read_only()
                        && !existing.access.can_write()
                        && !incoming.access.can_write())))
    }

    pub(super) fn access(&self) -> AccessMode {
        self.0.borrow().access
    }

    pub(crate) fn read(&self, buffer: &mut [u8]) -> Result<usize, DosError> {
        let mut open = self.0.borrow_mut();
        if !open.access.can_read() {
            return Err(DosError::AccessDenied);
        }
        let count = open
            .file
            .read_at(buffer, u64::from(open.position))
            .map_err(|_| DosError::AccessDenied)?;
        // A host file can grow beyond DOS's 32-bit offsets.
        open.position = open
            .position
            .checked_add(count as u32)
            .ok_or(DosError::AccessDenied)?;
        Ok(count)
    }

    pub(crate) fn write(&self, bytes: &[u8]) -> Result<usize, DosError> {
        let mut open = self.0.borrow_mut();
        if !open.access.can_write() {
            return Err(DosError::AccessDenied);
        }
        if bytes.is_empty() {
            // DOS AH=40h with CX=0 sets the file length at the current position.
            open.file
                .set_len(u64::from(open.position))
                .map_err(|_| DosError::AccessDenied)?;
            open.written = true;
            open.explicit_timestamp = false;
            return Ok(0);
        }
        // DOS cursors and file sizes remain 32-bit even though VFS offsets are u64.
        let count = u32::try_from(bytes.len()).map_err(|_| DosError::AccessDenied)?;
        open.position
            .checked_add(count)
            .ok_or(DosError::AccessDenied)?;
        let count = open
            .file
            .write_at(bytes, u64::from(open.position))
            .map_err(|_| DosError::AccessDenied)?;
        open.position += count as u32;
        open.written = true;
        open.explicit_timestamp = false;
        Ok(count)
    }

    pub(super) fn seek(&self, origin: SeekOrigin, offset: u32) -> Result<u32, DosError> {
        let mut open = self.0.borrow_mut();
        let base = match origin {
            SeekOrigin::Start => 0,
            SeekOrigin::Current => open.position,
            SeekOrigin::End => {
                u32::try_from(open.file.len()).map_err(|_| DosError::AccessDenied)?
            }
        };
        // Absolute offsets use all 32 bits; relative offsets wrap as DOS ADD/ADC does.
        open.position = base.wrapping_add(offset);
        Ok(open.position)
    }

    pub(super) fn commit(&self, filesystem: &mut FileSystem) {
        let mut open = self.0.borrow_mut();
        if !open.written {
            return;
        }
        if !open.explicit_timestamp {
            open.modified = filesystem.timestamp();
        }
        // VFS writes are immediate; commit and close publish DOS directory metadata.
        // A replaced mounted path must not receive the old open file's metadata.
        if filesystem
            .drives
            .get(open.path.drive)
            .unwrap()
            .open(&open.path.relative)
            .is_ok_and(|file| file.same_file(&open.file))
        {
            let entry = filesystem
                .entries
                .entry(open.path.clone())
                .or_insert(EntryMetadata {
                    attributes: 0,
                    modified: open.modified,
                });
            entry.attributes |= ARCHIVE_ATTRIBUTE as u8;
            entry.modified = open.modified;
        }
    }

    pub(super) fn timestamp(&self) -> FileTimestamp {
        self.0.borrow().modified
    }

    pub(super) fn set_timestamp(&self, timestamp: FileTimestamp) {
        let mut open = self.0.borrow_mut();
        open.modified = timestamp;
        open.explicit_timestamp = true;
        open.written = true;
    }

    pub(super) fn information(&self) -> u16 {
        const FILE_NOT_WRITTEN: u16 = 1 << 6;
        let open = self.0.borrow();
        u16::from(open.path.drive as u8 - b'A') | if open.written { 0 } else { FILE_NOT_WRITTEN }
    }

    pub(crate) fn input_available(&self) -> bool {
        let open = self.0.borrow();
        u64::from(open.position) < open.file.len()
    }

    pub(super) fn peek_character(&self) -> Result<Option<u8>, DosError> {
        let open = self.0.borrow();
        if !open.access.can_read() {
            return Err(DosError::AccessDenied);
        }
        let mut byte = [0];
        let count = open
            .file
            .read_at(&mut byte, u64::from(open.position))
            .map_err(|_| DosError::AccessDenied)?;
        Ok((count != 0).then_some(byte[0]))
    }
}
