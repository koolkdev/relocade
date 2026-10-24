mod transfers;
pub(crate) use transfers::Transfers;

use crate::{Error, MemoryAccess, Result, SharedMemoryStorage, WasmSharedMemory};
use js_sys::{Reflect, SharedArrayBuffer, WebAssembly};
use std::{fmt, rc::Rc};
use wasm_bindgen::JsCast;

/// A retained allocation on this JavaScript thread.
#[derive(Clone, Debug)]
pub struct Memory {
    pub(super) allocation: Rc<Allocation>,
}

pub(crate) struct Allocation {
    pub(super) memory: WebAssembly::Memory,
    transfers: Transfers,
}

impl fmt::Debug for Allocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Allocation")
            .field("memory", &self.memory)
            .finish_non_exhaustive()
    }
}

impl Memory {
    pub(super) fn new(memory: WebAssembly::Memory, shared: bool) -> Result<Self> {
        let transfers = Transfers::new(&memory, shared)?;
        Ok(Self {
            allocation: Rc::new(Allocation { memory, transfers }),
        })
    }

    #[inline]
    pub(super) fn access(&self) -> MemoryAccess<'_> {
        MemoryAccess::web(&self.allocation.transfers)
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
        Self::from_memory(memory).map_err(serde::de::Error::custom)
    }
}

impl SharedMemory {
    /// Accepts a shared memory, including one received through worker postMessage.
    /// Private memories cannot be read independently of their backend.
    pub fn from_memory(memory: WebAssembly::Memory) -> Result<Self> {
        if !memory.buffer().is_instance_of::<SharedArrayBuffer>() {
            return Err(Error::new(
                "import shared memory",
                "expected shared WebAssembly.Memory",
            ));
        }
        // Bind against this worker's Rust memory; only the engine memory resource
        // crosses the worker boundary.
        Ok(Self {
            memory: Memory::new(memory, true)?,
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
        self.memory.allocation.transfers.read(offset, bytes);
    }

    fn write(&self, offset: u32, bytes: &[u8]) {
        self.memory.allocation.transfers.write(offset, bytes);
    }
}

pub(super) fn size(memory: &WebAssembly::Memory) -> Result<u64> {
    let byte_length = Reflect::get(&memory.buffer(), &"byteLength".into())
        .map_err(|error| super::js_error("read Wasm memory size", error))?
        .as_f64()
        .expect("ArrayBuffer byteLength is a number") as u64;
    Ok(byte_length / 65536)
}
