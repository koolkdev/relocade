//! Access whose allocated extent is established once when the view is created.

use super::{Load, MemoryAccess, MemoryValue, Store};

/// A checked prefix of a live allocation. Its size cannot change while borrowed.
/// Fixed-width callers can use the type's extent instead of repeatedly checking
/// the allocation's dynamic length. Shared bytes retain their ordinary atomic
/// transfer rules; this view does not provide a consistent snapshot.
pub struct FixedMemoryAccess<'a, const N: usize> {
    memory: crate::backend::FixedMemoryRegion<'a, N>,
}

impl<'a> MemoryAccess<'a> {
    /// Restricts this view to its first `N` allocated bytes.
    #[inline]
    pub fn into_prefix<const N: usize>(self) -> FixedMemoryAccess<'a, N> {
        FixedMemoryAccess {
            memory: self.memory.into_prefix::<N>(),
        }
    }
}

impl<const N: usize> FixedMemoryAccess<'_, N> {
    #[inline]
    pub fn reborrow(&mut self) -> FixedMemoryAccess<'_, N> {
        FixedMemoryAccess {
            memory: self.memory.reborrow(),
        }
    }

    #[inline]
    pub fn read(&self, offset: u32, bytes: &mut [u8]) {
        Self::check(offset, bytes.len());
        self.memory.read(offset, bytes);
    }

    #[inline]
    pub fn write(&mut self, offset: u32, bytes: &[u8]) {
        Self::check(offset, bytes.len());
        self.memory.write(offset, bytes);
    }

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

    #[inline]
    fn check(offset: u32, len: usize) {
        let start = offset as usize;
        assert!(start <= N && len <= N - start, "fixed memory range");
    }
}

macro_rules! scalar {
    ($value:ty) => {
        impl<const N: usize> Load<$value> for FixedMemoryAccess<'_, N> {
            #[inline]
            fn load(&self, offset: u32) -> $value {
                Self::check(offset, size_of::<$value>());
                self.memory.load(offset)
            }
        }
        impl<const N: usize> Store<$value> for FixedMemoryAccess<'_, N> {
            #[inline]
            fn store(&mut self, offset: u32, value: $value) {
                Self::check(offset, size_of::<$value>());
                self.memory.store(offset, value);
            }
        }
    };
}
scalar!(u8);
scalar!(u16);
scalar!(u32);
scalar!(u64);
