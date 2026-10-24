#[cfg(target_arch = "wasm32")]
use super::Region;
use super::{MemoryAccess, MemoryValue};

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

macro_rules! scalar {
    ($type:ty) => {
        impl Load<$type> for MemoryAccess<'_> {
            #[inline]
            fn load(&self, offset: u32) -> $type {
                #[cfg(target_arch = "wasm32")]
                if let Region::Web(transfers) = &self.region {
                    return transfers.load(offset);
                }
                let mut bytes = <$type as MemoryValue>::Bytes::default();
                self.read(offset, &mut bytes);
                <$type>::from_le_bytes(bytes)
            }
        }

        impl Store<$type> for MemoryAccess<'_> {
            #[inline]
            fn store(&mut self, offset: u32, value: $type) {
                #[cfg(target_arch = "wasm32")]
                if let Region::Web(transfers) = &mut self.region {
                    transfers.store(offset, value);
                    return;
                }
                self.write(offset, &value.to_le_bytes());
            }
        }
    };
}

scalar!(u8);
scalar!(u16);
scalar!(u32);
scalar!(u64);
