//! Port operations installed in one CPU's function tables.

use std::rc::Rc;

pub(crate) type Read = dyn Fn(u16, u8) -> u32;
pub(crate) type Write = dyn Fn(u16, u8, u32);

/// A port read operation. Clones share captures and can serve many ports.
/// Captures stay on the execution worker and need not implement Send or Sync.
#[derive(Clone)]
pub struct PortRead(pub(crate) Rc<Read>);

impl PortRead {
    pub fn new(read: impl Fn(u16, u8) -> u32 + 'static) -> Self {
        Self(Rc::new(read))
    }

    /// Returns FF for every transferred byte, even over a connected device.
    pub fn open_bus() -> Self {
        Self::new(|_, _| u32::MAX)
    }
}

/// A port write operation. It receives the architectural transfer width
/// and only the low transferred bytes of the value.
#[derive(Clone)]
pub struct PortWrite(pub(crate) Rc<Write>);

impl PortWrite {
    pub fn new(write: impl Fn(u16, u8, u32) + 'static) -> Self {
        Self(Rc::new(write))
    }

    /// Ignores writes, even over a connected device.
    pub fn ignored() -> Self {
        Self::new(|_, _, _| {})
    }
}

pub(crate) fn width_mask(bytes: u8) -> u32 {
    assert!(matches!(bytes, 1 | 2 | 4), "port transfer width");
    u32::MAX >> ((4 - bytes) * 8)
}
