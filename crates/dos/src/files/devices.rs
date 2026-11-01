//! DOS character devices and the state shared by duplicated device handles.

use super::{handles::Handle, AccessMode, FileSystem, Files};
use crate::{devices::CharacterDevice, error::DosError, path, stdio::OutputStream};
use std::{cell::Cell, rc::Rc};

const CHARACTER_DEVICE: u8 = 0x80;
const INPUT_NOT_EOF: u8 = 0x40;
const RAW_MODE: u8 = 0x20;

pub(crate) struct DeviceHandle {
    pub(crate) device: Rc<CharacterDevice>,
    pub(crate) stream: OutputStream,
    pub(super) access: AccessMode,
    flags: Cell<u8>,
}

impl DeviceHandle {
    pub(super) fn new(
        device: Rc<CharacterDevice>,
        stream: OutputStream,
        access: AccessMode,
    ) -> Self {
        let flags = CHARACTER_DEVICE | INPUT_NOT_EOF | (device.attributes() as u8 & 0x1f);
        Self {
            device,
            stream,
            access,
            flags: Cell::new(flags),
        }
    }

    pub(super) fn information(&self) -> u16 {
        // DH reports the character-driver attribute; DL belongs to this open record.
        (self.device.attributes() & 0xff00) | u16::from(self.flags.get())
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
        let Some(device) = self.devices.find(stem) else {
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
        self.handles[handle] = Some(Handle::Device(Rc::new(DeviceHandle::new(
            device,
            OutputStream::Stdout,
            access,
        ))));
        Ok(Some(handle as u16))
    }
}
