//! DOS character devices and the state shared by duplicated device handles.

use super::{handles::Handle, AccessMode, FileSystem, Files};
use crate::{error::DosError, path, stdio::OutputStream};
use std::{cell::Cell, rc::Rc};

const CHARACTER_DEVICE: u8 = 0x80;
const INPUT_NOT_EOF: u8 = 0x40;
const RAW_MODE: u8 = 0x20;

#[derive(Clone, Copy)]
pub(crate) enum Device {
    Console(OutputStream),
    Null,
    Auxiliary,
    Printer,
}

pub(crate) struct DeviceHandle {
    pub(super) kind: Device,
    pub(super) access: AccessMode,
    flags: Cell<u8>,
}

impl DeviceHandle {
    pub(crate) fn kind(&self) -> Device {
        self.kind
    }

    pub(super) fn new(kind: Device, access: AccessMode) -> Self {
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

    pub(super) fn information(&self) -> u16 {
        // DH reports the character-driver attribute; DL belongs to this open record.
        0x8000 | u16::from(self.flags.get())
    }

    pub(crate) fn eof(&self) -> bool {
        self.flags.get() & INPUT_NOT_EOF == 0
    }

    pub(crate) fn raw(&self) -> bool {
        self.flags.get() & RAW_MODE != 0
    }

    pub(crate) fn set_eof(&self, eof: bool) {
        let flags = self.flags.get();
        self.flags.set(if eof {
            flags & !INPUT_NOT_EOF
        } else {
            flags | INPUT_NOT_EOF
        });
    }
}

impl Files {
    pub(crate) fn set_device_information(&self, handle: u16, info: u16) -> Result<u8, DosError> {
        let Handle::Device(device) = self.handle(handle)? else {
            return Err(DosError::InvalidFunction);
        };
        if info & 0xff00 != 0 {
            return Err(DosError::InvalidData);
        }
        // IOCTL cannot turn a device into a file. Duplicates share the other flags.
        Ok(device.flags.replace(info as u8 | CHARACTER_DEVICE))
    }

    pub(super) fn open_device(
        &mut self,
        filesystem: &FileSystem,
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
        let path = filesystem.file_path(path)?;
        if !filesystem
            .drives
            .get(path.drive)
            .unwrap()
            .is_dir(path::parent(&path.relative))
        {
            return Err(DosError::PathNotFound);
        }
        let handle = self.vacant_handle()?;
        self.handles[handle] = Some(Handle::Device(Rc::new(DeviceHandle::new(kind, access))));
        Ok(Some(handle as u16))
    }
}
