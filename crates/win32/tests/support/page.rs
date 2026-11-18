use relocade_win32::{Exception, GuestMemory};
use std::cell::RefCell;

pub const BASE: u32 = 0x7000_1000;

pub struct Page(pub RefCell<[u8; 64]>);

impl Page {
    fn range(
        &self,
        address: u32,
        len: usize,
        write: bool,
    ) -> Result<std::ops::Range<usize>, Exception> {
        let start = address.wrapping_sub(BASE) as usize;
        let end = start
            .checked_add(len)
            .filter(|end| *end <= self.0.borrow().len());
        Ok(start..end.ok_or(Exception::PageFault {
            linear_address: address,
            error_code: if write { 6 } else { 4 },
        })?)
    }
}

impl GuestMemory for Page {
    fn read(&self, address: u32, bytes: &mut [u8]) -> Result<(), Exception> {
        bytes.copy_from_slice(&self.0.borrow()[self.range(address, bytes.len(), false)?]);
        Ok(())
    }

    fn write(&self, address: u32, bytes: &[u8]) -> Result<(), Exception> {
        let range = self.range(address, bytes.len(), true)?;
        self.0.borrow_mut()[range].copy_from_slice(bytes);
        Ok(())
    }
}
