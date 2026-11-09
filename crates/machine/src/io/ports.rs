//! Port operations installed in one CPU's function tables.

use crate::PortMappings;
use std::rc::Rc;

pub(crate) type Read = dyn Fn(u16, u8) -> u32;
pub(crate) type ConfigureRead = dyn Fn(&mut PortMappings<'_>, u16, u8) -> u32;
pub(crate) type Write = dyn Fn(u16, u8, u32);
pub(crate) type ConfigureWrite = dyn Fn(&mut PortMappings<'_>, u16, u8, u32);

pub(crate) enum Operation<F: ?Sized, C: ?Sized> {
    Access(Rc<F>),
    Configure(Rc<C>),
}

impl<F: ?Sized, C: ?Sized> Clone for Operation<F, C> {
    fn clone(&self) -> Self {
        match self {
            Self::Access(handler) => Self::Access(handler.clone()),
            Self::Configure(handler) => Self::Configure(handler.clone()),
        }
    }
}

/// A port read operation. Clones share captures and can serve many ports.
/// Captures stay on the execution worker and need not implement Send or Sync.
#[derive(Clone)]
pub struct PortRead(pub(crate) Operation<Read, ConfigureRead>);

impl PortRead {
    /// Creates an ordinary read with no table-configuration work on its call path.
    pub fn new(read: impl Fn(u16, u8) -> u32 + 'static) -> Self {
        Self(Operation::Access(Rc::new(read)))
    }

    /// Creates a read that can change this CPU's port handlers, for example when
    /// reading a status register changes the meaning of another register.
    /// Changes apply immediately; the current invocation retains its captures.
    pub fn configuring(read: impl Fn(&mut PortMappings<'_>, u16, u8) -> u32 + 'static) -> Self {
        Self(Operation::Configure(Rc::new(read)))
    }

    /// Returns FF for every transferred byte, even over a connected device.
    pub fn open_bus() -> Self {
        Self::new(|_, _| u32::MAX)
    }
}

/// A port write operation. It receives the architectural transfer width
/// and only the low transferred bytes of the value.
#[derive(Clone)]
pub struct PortWrite(pub(crate) Operation<Write, ConfigureWrite>);

impl PortWrite {
    /// Creates an ordinary write with no table-configuration work on its call path.
    pub fn new(write: impl Fn(u16, u8, u32) + 'static) -> Self {
        Self(Operation::Access(Rc::new(write)))
    }

    /// Creates a write that can change this CPU's port handlers at a device
    /// configuration boundary. Changes apply immediately, including between REP
    /// elements; the current invocation retains its captures if it replaces itself.
    pub fn configuring(write: impl Fn(&mut PortMappings<'_>, u16, u8, u32) + 'static) -> Self {
        Self(Operation::Configure(Rc::new(write)))
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
