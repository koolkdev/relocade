/// A scalar stored in little-endian linear memory: `u8`, `u16`, `u32` or `u64`.
/// Its Rust type determines the load/store width without runtime size dispatch.
pub trait MemoryValue: sealed::MemoryValue {
    type Bytes: AsRef<[u8]> + AsMut<[u8]> + Default;

    fn from_le_bytes(bytes: Self::Bytes) -> Self;
    fn to_le_bytes(self) -> Self::Bytes;

    /// Decodes a value from a byte-oriented storage implementation.
    fn read_with(read: impl FnOnce(&mut [u8])) -> Self {
        let mut bytes = Self::Bytes::default();
        read(bytes.as_mut());
        Self::from_le_bytes(bytes)
    }
}

macro_rules! scalar {
    ($($type:ty),* $(,)?) => {$(
        impl MemoryValue for $type {
            type Bytes = [u8; size_of::<Self>()];

            #[inline]
            fn from_le_bytes(bytes: Self::Bytes) -> Self {
                Self::from_le_bytes(bytes)
            }

            #[inline]
            fn to_le_bytes(self) -> Self::Bytes {
                self.to_le_bytes()
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        impl sealed::MemoryValue for $type {}
    )*};
}

scalar!(u8, u16, u32, u64);

/// Access to linear memory through a Wasm backend or its scoped context.
/// Handles belong to that engine. Transfers require an allocated, in-bounds
/// range; violating this storage invariant is a programming error.
pub trait WasmMemoryAccess {
    /// A handle to engine-owned memory that host functions can capture. Access to
    /// the contents still requires the owning backend or its scoped context.
    type Memory: Clone + Send + Sync + 'static;

    fn read_memory(&self, memory: &Self::Memory, offset: u32, bytes: &mut [u8]);
    fn write_memory(&mut self, memory: &Self::Memory, offset: u32, bytes: &[u8]);

    /// Loads a little-endian value, including at unaligned offsets.
    /// The value type selects the transfer width at compile time.
    fn load_memory<V: MemoryValue>(&self, memory: &Self::Memory, offset: u32) -> V {
        V::read_with(|bytes| self.read_memory(memory, offset, bytes))
    }

    /// Stores a little-endian value with the same synchronization as byte transfers.
    fn store_memory<V: MemoryValue>(&mut self, memory: &Self::Memory, offset: u32, value: V) {
        self.write_memory(memory, offset, value.to_le_bytes().as_ref());
    }
}

impl<A: WasmMemoryAccess + ?Sized> WasmMemoryAccess for &mut A {
    type Memory = A::Memory;

    fn read_memory(&self, memory: &Self::Memory, offset: u32, bytes: &mut [u8]) {
        (**self).read_memory(memory, offset, bytes);
    }

    fn write_memory(&mut self, memory: &Self::Memory, offset: u32, bytes: &[u8]) {
        (**self).write_memory(memory, offset, bytes);
    }

    fn load_memory<V: MemoryValue>(&self, memory: &Self::Memory, offset: u32) -> V {
        (**self).load_memory(memory, offset)
    }

    fn store_memory<V: MemoryValue>(&mut self, memory: &Self::Memory, offset: u32, value: V) {
        (**self).store_memory(memory, offset, value);
    }
}

/// An independently owned handle for shared linear memory. Reads may overlap
/// guest or host writes and do not provide a consistent snapshot of a range.
/// The range must be allocated and in bounds, as with [`WasmMemoryAccess`].
/// Scalars are little-endian, may be unaligned, and provide no additional
/// atomicity or snapshot guarantee.
///
/// Native handles can cross threads. Browser handles stay on their JavaScript
/// thread; the underlying shared WebAssembly.Memory can be sent to a worker.
pub trait WasmSharedMemory: Clone + 'static {
    fn read(&self, offset: u32, bytes: &mut [u8]);
    /// Transfers bytes without excluding other readers, writers or guest execution.
    fn write(&self, offset: u32, bytes: &[u8]);
    fn byte_len(&self) -> u64;

    fn load<V: MemoryValue>(&self, offset: u32) -> V {
        V::read_with(|bytes| self.read(offset, bytes))
    }

    fn store<V: MemoryValue>(&self, offset: u32, value: V) {
        self.write(offset, value.to_le_bytes().as_ref());
    }
}

pub(crate) mod sealed {
    pub trait MemoryValue: Copy {
        #[cfg(target_arch = "wasm32")]
        fn load(transfers: &crate::web::memory::transfers::Transfers, offset: u32) -> Self;
        #[cfg(target_arch = "wasm32")]
        fn store(transfers: &crate::web::memory::transfers::Transfers, offset: u32, value: Self);
    }
}
