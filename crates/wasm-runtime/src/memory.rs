//! Borrowed access to one allocation, independent of store lookup and host state.

mod scalar;
mod value;
pub use scalar::{Load, Store};
pub use value::MemoryValue;

/// Access to linear memory through a Wasm backend or its scoped context.
/// Handles belong to that engine. Transfers require an allocated, in-bounds
/// range; violating this storage invariant is a programming error.
pub trait WasmMemoryAccess {
    /// A handle to engine-owned memory that host functions can capture. Access to
    /// the contents still requires the owning backend or its scoped context.
    type Memory: Clone + Send + Sync + 'static;

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
pub trait WasmSharedMemory: Clone + 'static {
    /// Borrows live read/write access without excluding other owners or growth.
    fn view(&self) -> MemoryAccess<'_>;
    fn read(&self, offset: u32, bytes: &mut [u8]);
    fn write(&self, offset: u32, bytes: &[u8]);
    fn byte_len(&self) -> u64;
}

/// Live access to an already bound allocation. Reborrowing does not clone its
/// resource handle or borrow the backend again. Private access excludes guest
/// execution through its backend borrow; shared access can overlap other readers,
/// writers and growth. Transfers must be allocated and in bounds.
pub struct MemoryAccess<'a> {
    memory: crate::backend::MemoryRegion<'a>,
}

impl<'a> MemoryAccess<'a> {
    pub(crate) fn new(memory: crate::backend::MemoryRegion<'a>) -> Self {
        Self { memory }
    }

    #[inline]
    pub fn reborrow(&mut self) -> MemoryAccess<'_> {
        MemoryAccess::new(self.memory.reborrow())
    }

    #[inline]
    pub fn read(&self, offset: u32, bytes: &mut [u8]) {
        self.memory.read(offset, bytes);
    }

    #[inline]
    pub fn write(&mut self, offset: u32, bytes: &[u8]) {
        self.memory.write(offset, bytes);
    }
}
