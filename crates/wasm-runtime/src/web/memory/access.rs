use super::Transfers;
use crate::{Load, MemoryValue, Store};
use std::cell::Ref;

/// Keeps the store borrowed when needed; an existing owner needs no extra guard.
pub(crate) enum MemoryRegion<'a> {
    Borrowed(&'a Transfers),
    Stored(Ref<'a, Transfers>),
}

pub(crate) type FixedMemoryRegion<'a, const N: usize> = MemoryRegion<'a>;

impl<'a> MemoryRegion<'a> {
    #[inline]
    pub(crate) fn into_prefix<const N: usize>(self) -> FixedMemoryRegion<'a, N> {
        self.transfers().check_prefix(N);
        self
    }
}

impl MemoryRegion<'_> {
    fn transfers(&self) -> &Transfers {
        match self {
            Self::Borrowed(transfers) => transfers,
            Self::Stored(transfers) => transfers,
        }
    }

    pub(crate) fn reborrow(&mut self) -> MemoryRegion<'_> {
        MemoryRegion::Borrowed(self.transfers())
    }

    #[inline]
    pub(crate) fn read(&self, offset: u32, bytes: &mut [u8]) {
        self.transfers().read(offset, bytes);
    }

    #[inline]
    pub(crate) fn write(&self, offset: u32, bytes: &[u8]) {
        self.transfers().write(offset, bytes);
    }
}

impl<V: MemoryValue> Load<V> for MemoryRegion<'_>
where
    for<'a> &'a Transfers: Load<V>,
{
    #[inline]
    fn load(&self, offset: u32) -> V {
        self.transfers().load(offset)
    }
}

impl<V: MemoryValue> Store<V> for MemoryRegion<'_>
where
    for<'a> &'a Transfers: Store<V>,
{
    #[inline]
    fn store(&mut self, offset: u32, value: V) {
        self.transfers().store(offset, value);
    }
}
