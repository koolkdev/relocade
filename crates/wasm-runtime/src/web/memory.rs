mod access;
mod transfers;
pub(crate) use access::MemoryRegion;
use transfers::Transfers;

use crate::{Error, MemoryAccess, Result, WasmSharedMemory};
use js_sys::{Object, Reflect, SharedArrayBuffer, WebAssembly};
use std::{fmt, rc::Rc};
use wasm_bindgen::JsCast;

/// A key to memory owned by one backend. Access requires that backend or its context.
#[derive(Clone, Debug)]
pub struct Memory(pub(super) usize);

pub(crate) struct Allocation {
    pub(super) memory: WebAssembly::Memory,
    pub(super) transfers: Transfers,
}

impl fmt::Debug for Allocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Allocation")
            .field("memory", &self.memory)
            .finish_non_exhaustive()
    }
}

impl Allocation {
    pub(super) fn new(memory: WebAssembly::Memory, shared: bool) -> Result<Self> {
        let transfers = Transfers::new(&memory, shared)?;
        Ok(Self { memory, transfers })
    }

    #[inline]
    pub(super) fn access(&self) -> MemoryAccess<'_> {
        MemoryAccess::new(MemoryRegion::Borrowed(&self.transfers))
    }
}

/// A shared allocation retained independently of its backend.
#[derive(Clone)]
pub struct SharedMemory {
    pub(super) allocation: Rc<Allocation>,
}

#[cfg(feature = "worker")]
impl web_thread::Post for SharedMemory {}

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
    /// Allocates zeroed shared memory independently of a backend, in 64-KiB pages.
    pub fn new(minimum: u32, maximum: u32) -> Result<Self> {
        Self::from_memory(allocate(minimum, Some(maximum), true)?)
    }

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
            allocation: Rc::new(Allocation::new(memory, true)?),
        })
    }

    /// The JavaScript resource to share with another worker using postMessage.
    /// This requires a cross-origin-isolated page.
    pub fn memory(&self) -> &WebAssembly::Memory {
        &self.allocation.memory
    }
}

impl WasmSharedMemory for SharedMemory {
    fn view(&self) -> MemoryAccess<'_> {
        self.allocation.access()
    }

    fn byte_len(&self) -> u64 {
        size(self.memory()).expect("shared Wasm memory size") * 65536
    }

    fn read(&self, offset: u32, bytes: &mut [u8]) {
        self.allocation.transfers.read(offset, bytes);
    }

    fn write(&self, offset: u32, bytes: &[u8]) {
        self.allocation.transfers.write(offset, bytes);
    }
}

pub(super) fn size(memory: &WebAssembly::Memory) -> Result<u64> {
    let byte_length = Reflect::get(&memory.buffer(), &"byteLength".into())
        .map_err(|error| super::js_error("read Wasm memory size", error))?
        .as_f64()
        .expect("ArrayBuffer byteLength is a number") as u64;
    Ok(byte_length / 65536)
}

pub(super) fn allocate(
    minimum: u32,
    maximum: Option<u32>,
    shared: bool,
) -> Result<WebAssembly::Memory> {
    let descriptor = Object::new();
    super::set(&descriptor, "initial", &minimum.into())?;
    super::set(&descriptor, "shared", &shared.into())?;
    if let Some(maximum) = maximum {
        super::set(&descriptor, "maximum", &maximum.into())?;
    }
    WebAssembly::Memory::new(&descriptor)
        .map_err(|error| super::js_error("create Wasm memory", error))
}

impl<V: crate::MemoryValue> crate::Load<V> for SharedMemory
where
    for<'a> &'a Transfers: crate::Load<V>,
{
    #[inline]
    fn load(&self, offset: u32) -> V {
        (&self.allocation.transfers).load(offset)
    }
}

impl<V: crate::MemoryValue> crate::Store<V> for &SharedMemory
where
    for<'a> &'a Transfers: crate::Store<V>,
{
    #[inline]
    fn store(&mut self, offset: u32, value: V) {
        (&self.allocation.transfers).store(offset, value);
    }
}
