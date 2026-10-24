//! Borrowed access to one allocation, independent of store lookup and host state.

mod scalar;
mod value;
pub use scalar::{Load, Store};
pub use value::MemoryValue;

/// Byte-addressed custom storage. Transfers must be in bounds and take effect
/// immediately; implementations must not cache snapshots or defer writes.
pub trait MemoryStorage {
    fn read(&self, offset: u32, bytes: &mut [u8]);
    fn write(&mut self, offset: u32, bytes: &[u8]);
}

/// Custom storage that permits independent readers and writers. Its owner
/// supplies any required synchronization; transfers need not be atomic snapshots.
pub trait SharedMemoryStorage {
    fn read(&self, offset: u32, bytes: &mut [u8]);
    fn write(&self, offset: u32, bytes: &[u8]);
}

/// Access to linear memory through a Wasm backend or its scoped context.
/// Handles belong to that engine. Transfers require an allocated, in-bounds
/// range; violating this storage invariant is a programming error.
pub trait WasmMemoryAccess {
    /// A handle to engine-owned memory that host functions can capture. Access to
    /// the contents still requires the owning backend or its scoped context.
    #[cfg(not(target_arch = "wasm32"))]
    type Memory: Clone + Send + Sync + 'static;
    #[cfg(target_arch = "wasm32")]
    type Memory: Clone + 'static;

    /// Binds the allocation for a stopped phase. Drop the view before entering
    /// Wasm through this backend or context again.
    ///
    /// A bound view cannot overlap guest execution:
    /// ```compile_fail
    /// use relocade_wasm_runtime::{Backend, Function, Memory, WasmMemoryAccess};
    /// fn overlap(backend: &mut Backend, memory: &Memory, function: &Function<Backend, fn()>) {
    ///     let access = backend.memory(memory);
    ///     function.call(backend, ());
    ///     access.read(0, &mut [0]);
    /// }
    /// ```
    fn memory<'a>(&'a mut self, memory: &'a Self::Memory) -> MemoryAccess<'a>;
}

impl<A: WasmMemoryAccess + ?Sized> WasmMemoryAccess for &mut A {
    type Memory = A::Memory;

    fn memory<'a>(&'a mut self, memory: &'a Self::Memory) -> MemoryAccess<'a> {
        (**self).memory(memory)
    }
}

/// An independently owned handle for shared linear memory. Reads may overlap
/// guest or host writes and do not provide a consistent snapshot of a range.
/// The range must be allocated and in bounds, as with [`WasmMemoryAccess`].
///
/// Native backend handles can cross threads. Browser handles stay on their JavaScript
/// thread; the underlying shared WebAssembly.Memory can be sent to a worker.
pub trait WasmSharedMemory: SharedMemoryStorage + Clone + 'static {
    /// Borrows live read/write access without excluding other owners or growth.
    fn view(&self) -> MemoryAccess<'_> {
        MemoryAccess::from_shared_storage(self)
    }
    fn byte_len(&self) -> u64;
}

/// Live access to an already bound allocation. Reborrowing does not clone its
/// resource handle or borrow the backend again. Private access excludes guest
/// execution through its backend borrow; shared access can overlap other readers,
/// writers and growth. Transfers must be allocated and in bounds.
pub struct MemoryAccess<'a> {
    region: Region<'a>,
}

enum Region<'a> {
    Bytes(&'a mut [u8]),
    Custom(&'a mut dyn MemoryStorage),
    SharedCustom(&'a dyn SharedMemoryStorage),
    #[cfg(not(target_arch = "wasm32"))]
    Shared(&'a crate::native::SharedMemory),
    #[cfg(target_arch = "wasm32")]
    Web(&'a crate::web::memory::Transfers),
}

impl<'a> MemoryAccess<'a> {
    #[inline]
    pub fn from_bytes(bytes: &'a mut [u8]) -> Self {
        Self {
            region: Region::Bytes(bytes),
        }
    }

    #[inline]
    pub fn from_storage(storage: &'a mut dyn MemoryStorage) -> Self {
        Self {
            region: Region::Custom(storage),
        }
    }

    #[inline]
    pub fn from_shared_storage(storage: &'a dyn SharedMemoryStorage) -> Self {
        Self {
            region: Region::SharedCustom(storage),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[inline]
    pub(crate) fn shared(memory: &'a crate::native::SharedMemory) -> Self {
        Self {
            region: Region::Shared(memory),
        }
    }

    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub(crate) fn web(memory: &'a crate::web::memory::Transfers) -> Self {
        Self {
            region: Region::Web(memory),
        }
    }

    #[inline]
    pub fn reborrow(&mut self) -> MemoryAccess<'_> {
        MemoryAccess {
            region: match &mut self.region {
                Region::Bytes(bytes) => Region::Bytes(bytes),
                Region::Custom(storage) => Region::Custom(*storage),
                Region::SharedCustom(storage) => Region::SharedCustom(*storage),
                #[cfg(not(target_arch = "wasm32"))]
                Region::Shared(memory) => Region::Shared(memory),
                #[cfg(target_arch = "wasm32")]
                Region::Web(memory) => Region::Web(memory),
            },
        }
    }

    #[inline]
    pub fn read(&self, offset: u32, bytes: &mut [u8]) {
        match &self.region {
            Region::Bytes(data) => bytes.copy_from_slice(&data[offset as usize..][..bytes.len()]),
            Region::Custom(storage) => storage.read(offset, bytes),
            Region::SharedCustom(storage) => storage.read(offset, bytes),
            #[cfg(not(target_arch = "wasm32"))]
            Region::Shared(memory) => memory.read(offset, bytes),
            #[cfg(target_arch = "wasm32")]
            Region::Web(memory) => memory.read(offset, bytes),
        }
    }

    #[inline]
    pub fn write(&mut self, offset: u32, bytes: &[u8]) {
        match &mut self.region {
            Region::Bytes(data) => data[offset as usize..][..bytes.len()].copy_from_slice(bytes),
            Region::Custom(storage) => storage.write(offset, bytes),
            Region::SharedCustom(storage) => storage.write(offset, bytes),
            #[cfg(not(target_arch = "wasm32"))]
            Region::Shared(memory) => memory.write(offset, bytes),
            #[cfg(target_arch = "wasm32")]
            Region::Web(memory) => memory.write(offset, bytes),
        }
    }
}
