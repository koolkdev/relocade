//! File creation, including unique temporary names, shares DOS handle policy.

use super::{
    AccessMode, Files, ARCHIVE_ATTRIBUTE, COMPATIBILITY_SHARING, HIDDEN_ATTRIBUTE,
    READ_ONLY_ATTRIBUTE, SYSTEM_ATTRIBUTE,
};
use crate::{error::DosError, path};
use std::io::ErrorKind;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum CreateMode {
    TruncateExisting,
    FailIfExists,
}

impl Files {
    pub(crate) fn create(
        &mut self,
        path: &[u8],
        attributes: u16,
        mode: CreateMode,
    ) -> Result<u16, DosError> {
        // DOS owns hidden/system/archive flags; VFS enforces read-only opens.
        if attributes
            & !(READ_ONLY_ATTRIBUTE | HIDDEN_ATTRIBUTE | SYSTEM_ATTRIBUTE | ARCHIVE_ATTRIBUTE)
            != 0
        {
            return Err(DosError::AccessDenied);
        }
        if let Some(handle) = self.open_device(path, AccessMode::ReadWrite)? {
            return Ok(handle);
        }
        let path = self.file_path(path)?;
        let filesystem = self.drives.get(path.drive).unwrap();
        if !filesystem.is_dir(path::parent(&path.relative)) {
            return Err(DosError::PathNotFound);
        }
        if mode == CreateMode::FailIfExists
            && filesystem
                .metadata(&path.relative)
                .is_ok_and(|entry| entry.is_file())
        {
            return Err(DosError::FileExists);
        }
        let file = match filesystem.open_read_write(&path.relative) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                // Check capacity before publishing a new file. It cannot share
                // identity with an existing handle, so sharing checks will pass.
                self.vacant_handle()?;
                self.drives
                    .get_mut(path.drive)
                    .unwrap()
                    .create_new(&path.relative)
                    .map_err(|_| DosError::AccessDenied)?
            }
            Err(_) => return Err(DosError::AccessDenied),
        };
        // Existing files are truncated only after sharing and capacity checks.
        let handle = self.open_file(
            file.clone(),
            path.clone(),
            AccessMode::ReadWrite,
            COMPATIBILITY_SHARING,
        )?;
        self.write(handle, &[])
            .expect("truncating a new writable handle does not allocate");
        file.set_read_only(attributes & READ_ONLY_ATTRIBUTE != 0);
        self.attributes
            .insert(path, (attributes | ARCHIVE_ATTRIBUTE) as u8);
        Ok(handle)
    }

    /// Replaces the supplied directory with the created path only on success.
    pub(crate) fn create_temporary(
        &mut self,
        path: &mut Vec<u8>,
        attributes: u16,
    ) -> Result<u16, DosError> {
        let mut candidate = path.clone();
        // DOS 5 inserts a missing separator, so an empty path names the root.
        if !matches!(candidate.last(), Some(b'\\' | b'/')) {
            candidate.push(b'\\');
        }
        let directory = self.resolve(&candidate)?;
        if !self
            .drives
            .get(directory.drive)
            .unwrap()
            .is_dir(&directory.relative)
        {
            return Err(DosError::PathNotFound);
        }
        let prefix_len = candidate.len();
        let first_name = self.next_temporary_name;
        loop {
            // Generate an eight-character name from the counter; skip existing names.
            let name = format!("{:08X}", self.next_temporary_name);
            self.next_temporary_name = self.next_temporary_name.wrapping_add(1);
            candidate.truncate(prefix_len);
            candidate.extend_from_slice(name.as_bytes());
            let canonical = self.file_path(&candidate)?;
            // A directory can occupy a generated name too.
            if self
                .drives
                .get(canonical.drive)
                .unwrap()
                .metadata(&canonical.relative)
                .is_err()
            {
                let handle = self.create(&candidate, attributes, CreateMode::FailIfExists)?;
                *path = candidate;
                return Ok(handle);
            }
            if self.next_temporary_name == first_name {
                return Err(DosError::AccessDenied);
            }
        }
    }
}
