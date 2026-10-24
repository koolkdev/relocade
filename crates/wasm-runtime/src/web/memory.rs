use crate::{MemoryAccess, Result, SharedMemoryStorage, WasmSharedMemory};
use js_sys::{Reflect, SharedArrayBuffer, Uint8Array, WebAssembly};
use std::rc::Rc;
use wasm_bindgen::JsCast;

/// A retained allocation on this JavaScript thread.
#[derive(Clone, Debug)]
pub struct Memory {
    pub(super) allocation: Rc<Allocation>,
}

#[derive(Debug)]
pub(crate) struct Allocation {
    pub(super) memory: WebAssembly::Memory,
}

impl Memory {
    pub(super) fn new(memory: WebAssembly::Memory) -> Self {
        Self {
            allocation: Rc::new(Allocation { memory }),
        }
    }

    #[inline]
    pub(super) fn access(&self) -> MemoryAccess<'_> {
        MemoryAccess::web(&self.allocation)
    }
}

impl Allocation {
    pub(crate) fn read(&self, offset: u32, bytes: &mut [u8]) {
        range(&self.memory, offset, bytes.len()).copy_to(bytes);
    }

    pub(crate) fn write(&self, offset: u32, bytes: &[u8]) {
        range(&self.memory, offset, bytes.len()).copy_from(bytes);
    }
}

/// A shared allocation retained independently of its backend.
#[derive(Clone)]
pub struct SharedMemory {
    pub(super) memory: Memory,
}

impl serde::Serialize for SharedMemory {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serde_wasm_bindgen::preserve::serialize(self.memory(), serializer)
    }
}

impl<'de> serde::Deserialize<'de> for SharedMemory {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let memory = serde_wasm_bindgen::preserve::deserialize(deserializer)?;
        Self::from_memory(memory)
            .ok_or_else(|| serde::de::Error::custom("expected shared WebAssembly.Memory"))
    }
}

impl SharedMemory {
    /// Accepts a shared memory, including one received through worker postMessage.
    /// Private memories cannot be read independently of their backend.
    pub fn from_memory(memory: WebAssembly::Memory) -> Option<Self> {
        memory
            .buffer()
            .is_instance_of::<SharedArrayBuffer>()
            .then(|| Self {
                memory: Memory::new(memory),
            })
    }

    /// The JavaScript resource to share with another worker using postMessage.
    /// This requires a cross-origin-isolated page.
    pub fn memory(&self) -> &WebAssembly::Memory {
        &self.memory.allocation.memory
    }
}

impl WasmSharedMemory for SharedMemory {
    fn view(&self) -> MemoryAccess<'_> {
        self.memory.access()
    }

    fn byte_len(&self) -> u64 {
        size(self.memory()).expect("shared Wasm memory size") * 65536
    }
}

impl SharedMemoryStorage for SharedMemory {
    fn read(&self, offset: u32, bytes: &mut [u8]) {
        self.memory.allocation.read(offset, bytes);
    }

    fn write(&self, offset: u32, bytes: &[u8]) {
        self.memory.allocation.write(offset, bytes);
    }
}

pub(super) fn range(memory: &WebAssembly::Memory, offset: u32, len: usize) -> Uint8Array {
    // Growth detaches private buffers and leaves old shared buffers with their
    // original length. Obtain the current buffer for every transfer.
    Uint8Array::new_with_byte_offset_and_length(
        &memory.buffer(),
        offset,
        len.try_into().expect("Wasm32 memory range"),
    )
}

pub(super) fn size(memory: &WebAssembly::Memory) -> Result<u64> {
    let byte_length = Reflect::get(&memory.buffer(), &"byteLength".into())
        .map_err(|error| super::js_error("read Wasm memory size", error))?
        .as_f64()
        .expect("ArrayBuffer byteLength is a number") as u64;
    Ok(byte_length / 65536)
}
