use crate::WasmSharedMemory;

/// Borrowed bytes of a Wasm memory. Access is live and ranges must be allocated.
/// A private-memory borrow prevents reentering or growing its owning backend.
/// Shared-memory borrows permit access through other owners and do not lock bytes.
pub struct MemoryBytes<'a> {
    buffer: Buffer<'a>,
}

enum Buffer<'a> {
    Slice(&'a mut [u8]),
    Shared(&'a dyn SharedBytes),
    #[cfg(target_arch = "wasm32")]
    Web(std::borrow::Cow<'a, js_sys::WebAssembly::Memory>),
}

// Erases the shared handle's type after its owner lends the allocation.
trait SharedBytes {
    fn read(&self, offset: u32, bytes: &mut [u8]);
    fn write(&self, offset: u32, bytes: &[u8]);
}

impl<S: WasmSharedMemory> SharedBytes for S {
    fn read(&self, offset: u32, bytes: &mut [u8]) {
        WasmSharedMemory::read(self, offset, bytes);
    }

    fn write(&self, offset: u32, bytes: &[u8]) {
        WasmSharedMemory::write(self, offset, bytes);
    }
}

impl<'a> MemoryBytes<'a> {
    pub fn new(bytes: &'a mut [u8]) -> Self {
        Self {
            buffer: Buffer::Slice(bytes),
        }
    }

    pub fn shared<S: WasmSharedMemory>(memory: &'a S) -> Self {
        Self {
            buffer: Buffer::Shared(memory),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn web(memory: js_sys::WebAssembly::Memory) -> Self {
        Self {
            buffer: Buffer::Web(std::borrow::Cow::Owned(memory)),
        }
    }

    /// Lends the same bytes for a shorter lifetime.
    pub fn reborrow(&mut self) -> MemoryBytes<'_> {
        MemoryBytes {
            buffer: match &mut self.buffer {
                Buffer::Slice(bytes) => Buffer::Slice(bytes),
                Buffer::Shared(memory) => Buffer::Shared(*memory),
                #[cfg(target_arch = "wasm32")]
                Buffer::Web(memory) => Buffer::Web(std::borrow::Cow::Borrowed(&**memory)),
            },
        }
    }

    pub fn read(&self, offset: u32, bytes: &mut [u8]) {
        match &self.buffer {
            Buffer::Slice(source) => {
                bytes.copy_from_slice(&source[offset as usize..][..bytes.len()])
            }
            Buffer::Shared(memory) => memory.read(offset, bytes),
            #[cfg(target_arch = "wasm32")]
            Buffer::Web(memory) => {
                crate::web::memory::range(memory, offset, bytes.len()).copy_to(bytes)
            }
        }
    }

    pub fn write(&mut self, offset: u32, bytes: &[u8]) {
        match &mut self.buffer {
            Buffer::Slice(destination) => {
                destination[offset as usize..][..bytes.len()].copy_from_slice(bytes)
            }
            Buffer::Shared(memory) => memory.write(offset, bytes),
            #[cfg(target_arch = "wasm32")]
            Buffer::Web(memory) => crate::web::memory::write(memory, offset, bytes),
        }
    }
}
