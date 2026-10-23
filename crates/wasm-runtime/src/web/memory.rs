mod access;
pub(crate) mod transfers;

use crate::{MemoryValue, Result, WasmSharedMemory};
use js_sys::{Reflect, SharedArrayBuffer, WebAssembly};
use std::rc::Rc;
use transfers::Transfers;
use wasm_bindgen::JsCast;

/// The resource and direct transfer functions share the same lifetime.
pub(super) struct Allocation {
    memory: WebAssembly::Memory,
    transfers: Transfers,
    shared: bool,
}

impl Allocation {
    pub(super) fn new(memory: WebAssembly::Memory) -> Result<Self> {
        let shared = memory.buffer().is_instance_of::<SharedArrayBuffer>();
        let transfers = Transfers::new(&memory, shared)?;
        Ok(Self {
            memory,
            transfers,
            shared,
        })
    }

    pub(super) fn memory(&self) -> &WebAssembly::Memory {
        &self.memory
    }
}

/// An independently owned handle for a shared WebAssembly.Memory.
#[derive(Clone)]
pub struct SharedMemory(Rc<Allocation>);

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
        if !memory.buffer().is_instance_of::<SharedArrayBuffer>() {
            return None;
        }
        Some(Self(Rc::new(
            Allocation::new(memory).expect("link shared memory transfers"),
        )))
    }

    pub(super) fn from_allocation(memory: &Rc<Allocation>) -> Option<Self> {
        memory.shared.then(|| Self(Rc::clone(memory)))
    }

    pub(super) fn allocation(&self) -> Rc<Allocation> {
        Rc::clone(&self.0)
    }

    /// The JavaScript resource to share with another worker using postMessage.
    /// This requires a cross-origin-isolated page.
    pub fn memory(&self) -> &WebAssembly::Memory {
        self.0.memory()
    }
}

impl WasmSharedMemory for SharedMemory {
    fn write(&self, offset: u32, bytes: &[u8]) {
        self.0.transfers.write(offset, bytes);
    }
    fn byte_len(&self) -> u64 {
        size(self.memory()).expect("shared Wasm memory size") * 65536
    }

    fn read(&self, offset: u32, bytes: &mut [u8]) {
        self.0.transfers.read(offset, bytes);
    }

    fn load<V: MemoryValue>(&self, offset: u32) -> V {
        self.0.transfers.load(offset)
    }

    fn store<V: MemoryValue>(&self, offset: u32, value: V) {
        self.0.transfers.store(offset, value);
    }
}

pub(super) fn size(memory: &WebAssembly::Memory) -> Result<u64> {
    let byte_length = Reflect::get(&memory.buffer(), &"byteLength".into())
        .map_err(|error| super::js_error("read Wasm memory size", error))?
        .as_f64()
        .expect("ArrayBuffer byteLength is a number") as u64;
    Ok(byte_length / 65536)
}
