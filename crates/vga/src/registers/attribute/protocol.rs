//! The Attribute Controller's address latch and alternating index/data writes.

use std::{cell::Cell, rc::Rc};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct AttributeProtocol {
    address: u8,
    // Snapshots copy the phase; restore preserves the allocation held by readers.
    phase: Rc<Cell<WritePhase>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WritePhase {
    Index,
    Data,
}

impl Clone for AttributeProtocol {
    fn clone(&self) -> Self {
        Self {
            address: self.address,
            phase: Rc::new(Cell::new(self.phase.get())),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.address = source.address;
        self.phase.set(source.phase.get());
    }
}

impl AttributeProtocol {
    pub fn new(address: u8) -> Self {
        Self {
            address: address & 0x3f,
            phase: Rc::new(Cell::new(WritePhase::Index)),
        }
    }

    pub fn address(&self) -> u8 {
        self.address
    }

    pub fn next_write(&self) -> WritePhase {
        self.phase.get()
    }

    /// Consume index writes and synchronously pass data writes to the handler.
    /// The sequence advances even when the handler ignores a data write.
    pub fn handle_write(&mut self, value: u8, write_data: impl FnOnce(u8, u8)) {
        match self.phase.get() {
            WritePhase::Index => {
                self.address = value & 0x3f;
                self.phase.set(WritePhase::Data);
            }
            WritePhase::Data => {
                self.phase.set(WritePhase::Index);
                write_data(self.address, value);
            }
        }
    }

    /// Prepare a read callback that restarts index selection after each read.
    /// The reader shares only the phase and does not borrow the protocol's owner.
    pub fn handle_read(&self, read: impl Fn() -> u8 + 'static) -> impl Fn() -> u8 + 'static {
        let phase = self.phase.clone();
        move || {
            let value = read();
            phase.set(WritePhase::Index);
            value
        }
    }

    pub fn to_state_bytes(&self) -> [u8; 2] {
        [self.address, u8::from(self.phase.get() == WritePhase::Data)]
    }

    pub fn from_state_bytes([address, phase]: [u8; 2]) -> Option<Self> {
        if address & !0x3f != 0 {
            return None;
        }
        let phase = match phase {
            0 => WritePhase::Index,
            1 => WritePhase::Data,
            _ => return None,
        };
        Some(Self {
            address,
            phase: Rc::new(Cell::new(phase)),
        })
    }
}
