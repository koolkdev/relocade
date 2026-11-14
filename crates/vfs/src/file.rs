use crate::space::{Allocation, Storage};
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
    allocation: Allocation,
}

impl File {
    pub(crate) fn new(contents: Vec<u8>, storage: &Rc<Storage>) -> io::Result<Self> {
        let allocation = storage.allocate(contents.len() as u64)?;
        Ok(Self {
            data: Rc::new(RefCell::new(FileData {
                contents,
                read_only: false,
                allocation,
            })),
            writable: true,
        })
    }

    pub(crate) fn replace(&mut self, contents: Vec<u8>, storage: &Rc<Storage>) -> io::Result<Self> {
        if let Some(data) = Rc::get_mut(&mut self.data) {
            // No handles retain this file, so its allocation can fund the replacement.
            let data = data.get_mut();
            data.allocation.resize(contents.len() as u64)?;
            data.contents = contents;
            data.read_only = false;
        } else {
            // Retained handles keep both the old contents and their allocation.
            *self = Self::new(contents, storage)?;
        }
        Ok(self.clone())
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
    /// A full volume permits a partial write; no room for any bytes returns StorageFull.
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
        checked_len(offset)?;
        let mut data = self.data.borrow_mut();
        let end = end.min(data.allocation.max_len());
        let count = end.saturating_sub(offset) as usize;
        if count == 0 {
            return Err(ErrorKind::StorageFull.into());
        }
        if end > data.contents.len() as u64 {
            data.resize(end)?;
        }
        data.contents[offset as usize..end as usize].copy_from_slice(&bytes[..count]);
        Ok(count)
    }

    /// Truncates the file or extends it with zeroes.
    /// Growth is all-or-nothing, returning StorageFull if it exceeds the volume's capacity.
    pub fn set_len(&self, len: u64) -> io::Result<()> {
        if !self.writable {
            return Err(ErrorKind::PermissionDenied.into());
        }
        self.data.borrow_mut().resize(len)
    }

    pub(crate) fn read_all(&self) -> Vec<u8> {
        self.data.borrow().contents.clone()
    }
}

impl FileData {
    fn resize(&mut self, len: u64) -> io::Result<()> {
        let len = checked_len(len)?;
        let old_len = self.contents.len();
        self.allocation.resize(len as u64)?;
        if len > old_len && self.contents.try_reserve(len - old_len).is_err() {
            self.allocation
                .resize(old_len as u64)
                .expect("restore previous allocation");
            return Err(ErrorKind::OutOfMemory.into());
        }
        self.contents.resize(len, 0);
        Ok(())
    }
}

fn checked_len(len: u64) -> io::Result<usize> {
    let len = usize::try_from(len).map_err(|_| ErrorKind::FileTooLarge)?;
    // Vec allocations cannot exceed isize::MAX bytes.
    if len > isize::MAX as usize {
        return Err(ErrorKind::FileTooLarge.into());
    }
    Ok(len)
}
