//! DOS character devices and the state shared by duplicated device handles.

use super::{handles::OpenHandle, AccessMode, Files};
use crate::{error::DosError, path, stdio::OutputStream};
use std::{cell::Cell, rc::Rc};

const CHARACTER_DEVICE: u8 = 0x80;
const INPUT_NOT_EOF: u8 = 0x40;

#[derive(Clone, Copy)]
pub(crate) enum Device {
    Console(OutputStream),
    Null,
    Auxiliary,
    Printer,
}

pub(super) struct OpenDevice {
    pub kind: Device,
    pub access: AccessMode,
    flags: Cell<u8>,
}

impl OpenDevice {
    pub fn new(kind: Device, access: AccessMode) -> Self {
        let attributes = match kind {
            Device::Console(_) => 0x03, // Standard input and output.
            Device::Null => 0x04,
            Device::Auxiliary | Device::Printer => 0,
        };
        Self {
            kind,
            access,
            flags: Cell::new(CHARACTER_DEVICE | INPUT_NOT_EOF | attributes),
        }
    }

    pub fn information(&self) -> u16 {
        // DH reports the character-driver attribute; DL belongs to this open record.
        0x8000 | u16::from(self.flags.get())
    }

    pub fn eof(&self) -> bool {
        self.flags.get() & INPUT_NOT_EOF == 0
    }

    pub fn set_eof(&self, eof: bool) {
        let flags = self.flags.get();
        self.flags.set(if eof {
            flags & !INPUT_NOT_EOF
        } else {
            flags | INPUT_NOT_EOF
        });
    }
}

impl Files {
    pub(super) fn open_device(
        &mut self,
        path: &[u8],
        access: AccessMode,
    ) -> Result<Option<u16>, DosError> {
        // DOS accepts an optional colon and ignores the extension in device names.
        let path = path.strip_suffix(b":").unwrap_or(path);
        let name = path
            .rsplit(|byte| matches!(byte, b'\\' | b'/'))
            .next()
            .unwrap();
        let name = if name.get(1) == Some(&b':') {
            &name[2..]
        } else {
            name
        };
        let stem = name.split(|&byte| byte == b'.').next().unwrap();
        let kind = if stem.eq_ignore_ascii_case(b"CON") {
            Device::Console(OutputStream::Stdout)
        } else if stem.eq_ignore_ascii_case(b"NUL") {
            Device::Null
        } else {
            return Ok(None);
        };
        let path = self.file_path(path)?;
        if !self
            .drives
            .get(path.drive)
            .unwrap()
            .is_dir(path::parent(&path.relative))
        {
            return Err(DosError::PathNotFound);
        }
        let handle = self.vacant_handle()?;
        self.handles[handle] = Some(OpenHandle::Device(Rc::new(OpenDevice::new(kind, access))));
        Ok(Some(handle as u16))
    }

    pub(crate) fn console_eof(&self, handle: u16) -> bool {
        self.console(handle).eof()
    }

    pub(crate) fn set_console_eof(&self, handle: u16, eof: bool) {
        self.console(handle).set_eof(eof);
    }

    fn console(&self, handle: u16) -> &OpenDevice {
        match self.handle(handle).expect("validated console handle") {
            OpenHandle::Device(device) if matches!(device.kind, Device::Console(_)) => device,
            _ => unreachable!("console handle"),
        }
    }
}
