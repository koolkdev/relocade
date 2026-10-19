//! In-memory file storage. Callers supply normalized paths separated by '/'.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// Whether a file may be opened for writing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileAccess {
    ReadOnly,
    ReadWrite,
}

#[derive(Default)]
pub(crate) struct FileSystem {
    files: BTreeMap<String, Rc<RefCell<File>>>,
}

impl FileSystem {
    pub(crate) fn mount(&mut self, path: String, contents: Vec<u8>, access: FileAccess) {
        self.insert(path, Rc::new(RefCell::new(File::new(contents, access))));
    }

    pub(crate) fn insert(&mut self, path: String, file: Rc<RefCell<File>>) {
        self.files.insert(path, file);
    }

    pub(crate) fn file(&self, path: &str) -> Option<&Rc<RefCell<File>>> {
        self.files.get(path)
    }

    pub(crate) fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        self.file(path).map(|file| file.borrow().contents.clone())
    }

    pub(crate) fn parent_exists(&self, path: &str) -> bool {
        self.is_directory(path.rsplit_once('/').map_or("", |(parent, _)| parent))
    }

    pub(crate) fn is_directory(&self, path: &str) -> bool {
        let prefix = format!("{path}/");
        path.is_empty() || self.files.keys().any(|name| name.starts_with(&prefix))
    }
}

pub(crate) struct File {
    contents: Vec<u8>,
    pub(crate) access: FileAccess,
}

#[derive(Debug)]
pub(crate) enum StorageError {
    FileTooLarge,
    OutOfMemory,
}

impl File {
    pub(crate) fn new(contents: Vec<u8>, access: FileAccess) -> Self {
        Self { contents, access }
    }

    pub(crate) fn len(&self) -> u32 {
        self.contents.len() as u32
    }

    pub(crate) fn read_at(&self, offset: u32, buffer: &mut [u8]) -> usize {
        let remaining = self.contents.get(offset as usize..).unwrap_or_default();
        let count = buffer.len().min(remaining.len());
        buffer[..count].copy_from_slice(&remaining[..count]);
        count
    }

    pub(crate) fn write_at(&mut self, offset: u32, bytes: &[u8]) -> Result<usize, StorageError> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let count = u32::try_from(bytes.len()).map_err(|_| StorageError::FileTooLarge)?;
        let end = offset
            .checked_add(count)
            .ok_or(StorageError::FileTooLarge)?;
        if end > self.len() {
            self.set_len(end)?;
        }
        self.contents[offset as usize..end as usize].copy_from_slice(bytes);
        Ok(bytes.len())
    }

    pub(crate) fn set_len(&mut self, len: u32) -> Result<(), StorageError> {
        let len = len as usize;
        if len > self.contents.len() {
            self.contents
                .try_reserve(len - self.contents.len())
                .map_err(|_| StorageError::OutOfMemory)?;
        }
        self.contents.resize(len, 0);
        Ok(())
    }
}
