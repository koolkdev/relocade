//! DOS open handles, shared cursors and I/O.

use super::{Files, ARCHIVE_ATTRIBUTE};
use crate::{error::DosError, path::ResolvedPath, stdio::OutputStream};
use relocade_vfs::File;
use std::{
    cell::{Cell, RefCell, RefMut},
    future::{pending, Future},
    rc::Rc,
};

// A DOS process starts with 20 handles, including its five standard devices.
pub(super) const HANDLE_COUNT: usize = 20;

#[derive(Clone)]
pub(super) enum OpenHandle {
    // Separate opens have separate cursors; duplicated handles share this record.
    File(Rc<RefCell<OpenFile>>),
    ConsoleInput { eof: Rc<Cell<bool>> },
    ConsoleOutput(OutputStream),
    Auxiliary,
    Printer,
}

pub(super) struct OpenFile {
    file: File,
    path: ResolvedPath,
    position: u32,
    readable: bool,
    writable: bool,
    deny_read: bool,
    deny_write: bool,
    compatibility: bool,
    written: bool,
}

impl Files {
    pub(super) fn is_open(&self, file: &File) -> bool {
        // DOS sharing keeps an open file's name alive until its last handle closes.
        self.handles.iter().flatten().any(
            |handle| matches!(handle, OpenHandle::File(open) if open.borrow().file.same_file(file)),
        )
    }

    pub(super) fn open_file(
        &mut self,
        file: File,
        path: ResolvedPath,
        mode: u8,
    ) -> Result<u16, DosError> {
        if file.len() > u64::from(u32::MAX) {
            return Err(DosError::AccessDenied);
        }
        let entry = OpenFile {
            file,
            path,
            position: 0,
            readable: mode & 3 != 1,
            writable: mode & 3 != 0,
            deny_read: matches!(mode & 0x70, 0x10 | 0x30),
            deny_write: matches!(mode & 0x70, 0x10 | 0x20),
            compatibility: mode & 0x70 == 0,
            written: false,
        };
        let read_only = entry.file.is_read_only();
        if self.handles.iter().flatten().any(|handle| {
            let OpenHandle::File(open) = handle else {
                return false;
            };
            let open = open.borrow();
            open.file.same_file(&entry.file)
                && ((entry.readable && open.deny_read)
                    || (entry.writable && open.deny_write)
                    // New deny-read/write modes exempt this process's existing accesses.
                    || (entry.deny_read && entry.deny_write)
                    // Compatibility and explicit sharing may mix only for read-only files.
                    || (open.compatibility != entry.compatibility
                        && !(read_only && !open.writable && !entry.writable)))
        }) {
            return Err(DosError::AccessDenied);
        }
        let handle = self.vacant_handle()?;
        self.handles[handle] = Some(OpenHandle::File(Rc::new(RefCell::new(entry))));
        Ok(handle as u16)
    }

    pub(super) fn vacant_handle(&self) -> Result<usize, DosError> {
        self.handles
            .iter()
            .position(Option::is_none)
            .ok_or(DosError::TooManyOpenFiles)
    }

    /// Reads one raw character from a console or redirected file handle.
    pub(crate) fn read_character(&mut self, handle: u16) -> Result<Option<u8>, DosError> {
        if self.is_console_input(handle)? {
            return Ok(self.stdio.input.try_read());
        }
        let mut byte = [0];
        Ok((self.read(handle, &mut byte)? != 0).then_some(byte[0]))
    }

    /// Waits after an empty character read. Redirected EOF cannot consume console input.
    pub(crate) fn wait_for_character(
        &self,
        handle: u16,
    ) -> Result<impl Future<Output = u8> + 'static, DosError> {
        let input = self
            .is_console_input(handle)?
            .then(|| self.stdio.input.read_character());
        Ok(async move {
            match input {
                Some(input) => input.await,
                // AH=07h cannot report EOF. File growth notifications are not supported.
                None => pending().await,
            }
        })
    }

    pub(crate) fn is_console_input(&self, handle: u16) -> Result<bool, DosError> {
        Ok(matches!(
            self.handle(handle)?,
            OpenHandle::ConsoleInput { .. }
        ))
    }

    /// EOF belongs to the open console input record, including all its duplicates.
    pub(crate) fn console_eof(&self, handle: u16) -> &Cell<bool> {
        match self.handle(handle).expect("validated console input handle") {
            OpenHandle::ConsoleInput { eof } => eof,
            _ => unreachable!("console input handle"),
        }
    }

    pub(crate) fn read(&mut self, handle: u16, buffer: &mut [u8]) -> Result<usize, DosError> {
        let mut open = self.file_mut(handle)?;
        if !open.readable {
            return Err(DosError::AccessDenied);
        }
        let count = open
            .file
            .read_at(buffer, u64::from(open.position))
            .map_err(|_| DosError::AccessDenied)?;
        // A host File handle can grow a mounted file beyond DOS's 32-bit offsets.
        open.position = open
            .position
            .checked_add(count as u32)
            .ok_or(DosError::AccessDenied)?;
        Ok(count)
    }

    /// Character output preserves console EOF; only AH=40h handle writes clear it.
    pub(crate) fn write_characters(&mut self, handle: u16, bytes: &[u8]) -> Result<(), DosError> {
        if self.is_console_input(handle)? {
            self.stdio.output(OutputStream::Stdout).extend(bytes);
        } else if !bytes.is_empty() {
            self.write(handle, bytes)?;
        }
        Ok(())
    }

    pub(crate) fn write(&mut self, handle: u16, bytes: &[u8]) -> Result<usize, DosError> {
        match self.handle(handle)? {
            OpenHandle::ConsoleInput { eof } => {
                // CON accepts output through its input handle too. Even a zero-byte
                // handle write clears EOF on that open record and its duplicates.
                eof.set(false);
                self.stdio.output(OutputStream::Stdout).extend(bytes);
                return Ok(bytes.len());
            }
            OpenHandle::ConsoleOutput(stream) => {
                self.stdio.output(*stream).extend(bytes);
                return Ok(bytes.len());
            }
            _ => {}
        }
        let mut open = self.file_mut(handle)?;
        if !open.writable {
            return Err(DosError::AccessDenied);
        }
        if bytes.is_empty() {
            // DOS AH=40h with CX=0 sets the file length at the current position.
            open.file
                .set_len(u64::from(open.position))
                .map_err(|_| DosError::AccessDenied)?;
            open.written = true;
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
        Ok(count)
    }

    pub(crate) fn seek(&mut self, handle: u16, origin: u8, offset: u32) -> Result<u32, DosError> {
        let mut open = self.file_mut(handle)?;
        let base = match origin {
            0 => 0,
            1 => open.position,
            2 => u32::try_from(open.file.len()).map_err(|_| DosError::AccessDenied)?,
            _ => return Err(DosError::InvalidFunction),
        };
        // Absolute offsets use all 32 bits; relative offsets wrap as DOS ADD/ADC does.
        open.position = base.wrapping_add(offset);
        Ok(open.position)
    }

    pub(crate) fn close(&mut self, handle: u16) -> Result<(), DosError> {
        let open = self
            .handles
            .get_mut(usize::from(handle))
            .and_then(Option::take)
            .ok_or(DosError::InvalidHandle)?;
        self.commit_handle(open);
        Ok(())
    }

    pub(crate) fn commit(&mut self, handle: u16) -> Result<(), DosError> {
        let open = self.handle(handle)?.clone();
        self.commit_handle(open);
        Ok(())
    }

    fn commit_handle(&mut self, handle: OpenHandle) {
        let OpenHandle::File(open) = handle else {
            return;
        };
        // VFS writes are immediate; commit and close publish DOS archive metadata.
        let open = open.borrow();
        // The host may have replaced this path while the old file was still open.
        if open.written
            && self
                .drives
                .get(open.path.drive)
                .unwrap()
                .open(&open.path.relative)
                .is_ok_and(|file| file.same_file(&open.file))
        {
            *self.attributes.entry(open.path.clone()).or_default() |= ARCHIVE_ATTRIBUTE as u8;
        }
    }

    pub(crate) fn duplicate(&mut self, source: u16) -> Result<u16, DosError> {
        let open = self.handle(source)?.clone();
        let destination = self.vacant_handle()?;
        self.handles[destination] = Some(open);
        Ok(destination as u16)
    }

    pub(crate) fn force_duplicate(
        &mut self,
        source: u16,
        destination: u16,
    ) -> Result<(), DosError> {
        let open = self.handle(source)?.clone();
        if source == destination {
            return Ok(());
        }
        let slot = self
            .handles
            .get_mut(usize::from(destination))
            .ok_or(DosError::InvalidHandle)?;
        if let Some(replaced) = slot.replace(open) {
            self.commit_handle(replaced);
        }
        Ok(())
    }

    pub(crate) fn device_information(&self, handle: u16) -> Result<u16, DosError> {
        const FILE_NOT_WRITTEN: u16 = 1 << 6;
        // Bit 7 identifies a device handle; DH includes the character-driver attribute.
        const CHARACTER_DEVICE: u16 = (1 << 15) | (1 << 7);
        const INPUT_NOT_EOF: u16 = 1 << 6;
        const STANDARD_INPUT: u16 = 1;
        const STANDARD_OUTPUT: u16 = 2;
        Ok(match self.handle(handle)? {
            OpenHandle::File(file) => {
                let file = file.borrow();
                u16::from(file.path.drive as u8 - b'A')
                    | if file.written { 0 } else { FILE_NOT_WRITTEN }
            }
            // Console handles describe CON, regardless of their access direction.
            OpenHandle::ConsoleInput { eof } => {
                CHARACTER_DEVICE
                    | STANDARD_INPUT
                    | STANDARD_OUTPUT
                    | if eof.get() { 0 } else { INPUT_NOT_EOF }
            }
            OpenHandle::ConsoleOutput(_) => {
                CHARACTER_DEVICE | INPUT_NOT_EOF | STANDARD_INPUT | STANDARD_OUTPUT
            }
            OpenHandle::Auxiliary | OpenHandle::Printer => CHARACTER_DEVICE | INPUT_NOT_EOF,
        })
    }

    pub(crate) fn reset_handles(&mut self) {
        self.close_all();
        self.stdio.clear();
        self.search_directories.clear();
        for (slot, device) in self.handles.iter_mut().zip([
            OpenHandle::ConsoleInput { eof: Rc::default() },
            OpenHandle::ConsoleOutput(OutputStream::Stdout),
            OpenHandle::ConsoleOutput(OutputStream::Stderr),
            OpenHandle::Auxiliary,
            OpenHandle::Printer,
        ]) {
            *slot = Some(device);
        }
    }

    pub(crate) fn close_all(&mut self) {
        for handle in std::mem::take(&mut self.handles).into_iter().flatten() {
            self.commit_handle(handle);
        }
    }

    fn handle(&self, handle: u16) -> Result<&OpenHandle, DosError> {
        self.handles
            .get(usize::from(handle))
            .and_then(Option::as_ref)
            .ok_or(DosError::InvalidHandle)
    }

    fn file_mut(&self, handle: u16) -> Result<RefMut<'_, OpenFile>, DosError> {
        match self.handle(handle)? {
            OpenHandle::File(file) => Ok(file.borrow_mut()),
            _ => Err(DosError::AccessDenied),
        }
    }
}
