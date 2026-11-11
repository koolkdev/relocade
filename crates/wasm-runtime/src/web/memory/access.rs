use super::Transfers;
use crate::{Load, MemoryValue, Store};

/// Borrows a retained allocation for the current engine access scope.
pub(crate) struct MemoryRegion<'a>(pub(super) &'a Transfers);

impl MemoryRegion<'_> {
    pub(crate) fn reborrow(&mut self) -> MemoryRegion<'_> {
        MemoryRegion(self.0)
    }

    #[inline]
    pub(crate) fn read(&self, offset: u32, bytes: &mut [u8]) {
        self.0.read(offset, bytes);
    }

    #[inline]
    pub(crate) fn write(&self, offset: u32, bytes: &[u8]) {
        self.0.write(offset, bytes);
    }
}

impl<V: MemoryValue> Load<V> for MemoryRegion<'_>
where
    for<'a> &'a Transfers: Load<V>,
{
    #[inline]
    fn load(&self, offset: u32) -> V {
        self.0.load(offset)
    }
}

impl<V: MemoryValue> Store<V> for MemoryRegion<'_>
where
    for<'a> &'a Transfers: Store<V>,
{
    #[inline]
    fn store(&mut self, offset: u32, value: V) {
        self.0.store(offset, value);
    }
}
