use super::{MemoryAccess, MemoryValue, Region};

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
    ($type:ty, $load:ident, $store:ident) => {
        impl Load<$type> for MemoryAccess<'_> {
            #[inline]
            fn load(&self, offset: u32) -> $type {
                match &self.region {
                    Region::Custom(storage) => return storage.$load(offset),
                    Region::SharedCustom(storage) => return storage.$load(offset),
                    #[cfg(target_arch = "wasm32")]
                    Region::Web(transfers) => return transfers.load(offset),
                    _ => {}
                }
                let mut bytes = <$type as MemoryValue>::Bytes::default();
                self.read(offset, &mut bytes);
                <$type>::from_le_bytes(bytes)
            }
        }

        impl Store<$type> for MemoryAccess<'_> {
            #[inline]
            fn store(&mut self, offset: u32, value: $type) {
                match &mut self.region {
                    Region::Custom(storage) => return storage.$store(offset, value),
                    Region::SharedCustom(storage) => return storage.$store(offset, value),
                    #[cfg(target_arch = "wasm32")]
                    Region::Web(transfers) => return transfers.store(offset, value),
                    _ => {}
                }
                self.write(offset, &value.to_le_bytes());
            }
        }
    };
}

scalar!(u8, load_u8, store_u8);
scalar!(u16, load_u16, store_u16);
scalar!(u32, load_u32, store_u32);
scalar!(u64, load_u64, store_u64);
