use super::{MemoryAccess, MemoryValue};
use crate::SharedMemory;

/// Loads one little-endian scalar from an allocated, in-bounds range.
/// Shared access does not promise an atomic snapshot of the value.
pub trait Load<V: MemoryValue> {
    fn load(&self, offset: u32) -> V;
}

/// Stores one little-endian scalar into an allocated, in-bounds range.
/// Shared access does not promise an atomic update of the value.
pub trait Store<V: MemoryValue> {
    fn store(&mut self, offset: u32, value: V);
}

impl MemoryAccess<'_> {
    #[inline]
    pub fn load<V: MemoryValue>(&self, offset: u32) -> V
    where
        Self: Load<V>,
    {
        Load::load(self, offset)
    }

    #[inline]
    pub fn store<V: MemoryValue>(&mut self, offset: u32, value: V)
    where
        Self: Store<V>,
    {
        Store::store(self, offset, value);
    }
}

impl SharedMemory {
    /// Loads one little-endian scalar without borrowing a backend.
    /// Shared transfers do not promise an atomic snapshot of the value.
    #[inline]
    pub fn load<V: MemoryValue>(&self, offset: u32) -> V
    where
        Self: Load<V>,
    {
        Load::load(self, offset)
    }

    /// Stores one little-endian scalar without borrowing a backend.
    /// Shared transfers do not promise an atomic update of the value.
    #[inline]
    pub fn store<V: MemoryValue>(&self, offset: u32, value: V)
    where
        for<'a> &'a Self: Store<V>,
    {
        Store::store(&mut &*self, offset, value);
    }
}

macro_rules! scalar {
    ($type:ty) => {
        impl Load<$type> for MemoryAccess<'_> {
            #[inline]
            fn load(&self, offset: u32) -> $type {
                self.memory.load(offset)
            }
        }

        impl Store<$type> for MemoryAccess<'_> {
            #[inline]
            fn store(&mut self, offset: u32, value: $type) {
                self.memory.store(offset, value);
            }
        }
    };
}

scalar!(u8);
scalar!(u16);
scalar!(u32);
scalar!(u64);
