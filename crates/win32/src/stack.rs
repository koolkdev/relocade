use crate::{Argument, Exception, Promoted};

/// Access to the stopped guest's flat virtual address space. Each operation
/// checks its complete span. A failed write leaves guest memory untouched.
pub trait GuestMemory {
    fn read(&self, address: u32, bytes: &mut [u8]) -> Result<(), Exception>;
    fn write(&mut self, address: u32, bytes: &[u8]) -> Result<(), Exception>;
}

/// Reads fixed parameters in declaration order, starting after the near return address.
pub struct StackReader<'a> {
    memory: &'a dyn GuestMemory,
    next: u32,
}

impl<'a> StackReader<'a> {
    pub fn at_entry(memory: &'a dyn GuestMemory, esp: u32) -> Self {
        Self {
            memory,
            next: esp.wrapping_add(4),
        }
    }

    pub fn read<T: Argument>(&mut self) -> Result<T, Exception> {
        let mut slot = T::Slot::default();
        self.memory.read(self.next, slot.as_mut())?;
        self.next = self.next.wrapping_add(slot.as_ref().len() as u32);
        Ok(T::decode(slot))
    }

    /// Captures the first unnamed argument after reading the fixed parameters.
    pub fn varargs(self) -> VaList {
        VaList(self.next)
    }
}

/// A guest cdecl argument cursor. Reading a faulting argument leaves it current.
#[derive(Clone, Copy, Debug)]
pub struct VaList(u32);

impl VaList {
    pub fn read<T: Promoted>(&mut self, memory: &dyn GuestMemory) -> Result<T, Exception> {
        let mut reader = StackReader {
            memory,
            next: self.0,
        };
        let value = reader.read()?;
        self.0 = reader.next;
        Ok(value)
    }
}

/// Fixed parameters in declaration order. Derive with [`crate::Win32Args`] on
/// a record of guest values. `STACK_BYTES` includes each parameter's slot padding.
pub trait Arguments: Sized {
    const STACK_BYTES: u32;
    fn read(stack: &mut StackReader<'_>) -> Result<Self, Exception>;
    fn encode(&self, bytes: &mut Vec<u8>);
}

impl Arguments for () {
    const STACK_BYTES: u32 = 0;
    fn read(_: &mut StackReader<'_>) -> Result<Self, Exception> {
        Ok(())
    }
    fn encode(&self, _: &mut Vec<u8>) {}
}
