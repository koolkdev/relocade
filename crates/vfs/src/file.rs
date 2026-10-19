use std::cell::RefCell;
use std::io::{self, ErrorKind};
use std::rc::Rc;

/// An open file with positioned I/O and no shared cursor.
/// Clones refer to the same contents and retain the handle's access rights.
#[derive(Clone, Debug)]
pub struct File {
    data: Rc<RefCell<FileData>>,
    writable: bool,
}

#[derive(Debug)]
struct FileData {
    contents: Vec<u8>,
    read_only: bool,
}

impl File {
    pub(crate) fn new(contents: Vec<u8>) -> Self {
        Self {
            data: Rc::new(RefCell::new(FileData {
                contents,
                read_only: false,
            })),
            writable: true,
        }
    }

    pub(crate) fn open(&self, writable: bool) -> io::Result<Self> {
        if writable && self.is_read_only() {
            return Err(ErrorKind::PermissionDenied.into());
        }
        Ok(Self {
            data: Rc::clone(&self.data),
            writable,
        })
    }

    /// Tests file identity independently of the path used to open it.
    pub fn same_file(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.data, &other.data)
    }

    pub fn len(&self) -> u64 {
        self.data.borrow().contents.len() as u64
    }

    pub fn is_empty(&self) -> bool {
        self.data.borrow().contents.is_empty()
    }

    /// Whether future opens for writing are denied.
    pub fn is_read_only(&self) -> bool {
        self.data.borrow().read_only
    }

    /// Changes permissions for future opens. Existing handles keep their rights.
    pub fn set_read_only(&self, read_only: bool) {
        self.data.borrow_mut().read_only = read_only;
    }

    /// Reads up to the buffer length. Offsets at or beyond EOF return zero.
    pub fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
        let data = self.data.borrow();
        if offset >= data.contents.len() as u64 {
            return Ok(0);
        }
        let remaining = &data.contents[offset as usize..];
        let count = buffer.len().min(remaining.len());
        buffer[..count].copy_from_slice(&remaining[..count]);
        Ok(count)
    }

    /// Writes bytes, extending the file with zeroes when necessary.
    /// Empty writes leave the length unchanged; use [`Self::set_len`] to resize.
    pub fn write_at(&self, bytes: &[u8], offset: u64) -> io::Result<usize> {
        if !self.writable {
            return Err(ErrorKind::PermissionDenied.into());
        }
        if bytes.is_empty() {
            return Ok(0);
        }
        let end = offset
            .checked_add(bytes.len() as u64)
            .ok_or(ErrorKind::FileTooLarge)?;
        let mut data = self.data.borrow_mut();
        if end > data.contents.len() as u64 {
            resize(&mut data.contents, end)?;
        }
        data.contents[offset as usize..end as usize].copy_from_slice(bytes);
        Ok(bytes.len())
    }

    /// Truncates the file or extends it with zeroes.
    pub fn set_len(&self, len: u64) -> io::Result<()> {
        if !self.writable {
            return Err(ErrorKind::PermissionDenied.into());
        }
        resize(&mut self.data.borrow_mut().contents, len)
    }

    pub(crate) fn read_all(&self) -> Vec<u8> {
        self.data.borrow().contents.clone()
    }
}

fn resize(contents: &mut Vec<u8>, len: u64) -> io::Result<()> {
    let len = usize::try_from(len).map_err(|_| ErrorKind::FileTooLarge)?;
    // Vec allocations cannot exceed isize::MAX bytes.
    if len > isize::MAX as usize {
        return Err(ErrorKind::FileTooLarge.into());
    }
    if len > contents.len() {
        contents
            .try_reserve(len - contents.len())
            .map_err(|_| ErrorKind::OutOfMemory)?;
    }
    contents.resize(len, 0);
    Ok(())
}
