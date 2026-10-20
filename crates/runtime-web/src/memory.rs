use js_sys::{Reflect, SharedArrayBuffer, Uint8Array, WebAssembly};
use relocade_runtime::{wasm::WasmSharedMemory, Result};
use wasm_bindgen::JsCast;

/// An independent reader for a shared WebAssembly.Memory.
#[derive(Clone)]
pub struct SharedMemory(WebAssembly::Memory);

impl SharedMemory {
    /// Accepts a shared memory, including one received through worker postMessage.
    /// Private memories cannot be read independently of their backend.
    pub fn from_memory(memory: WebAssembly::Memory) -> Option<Self> {
        memory
            .buffer()
            .is_instance_of::<SharedArrayBuffer>()
            .then_some(Self(memory))
    }

    /// The JavaScript resource to share with another worker using postMessage.
    /// This requires a cross-origin-isolated page.
    pub fn memory(&self) -> &WebAssembly::Memory {
        &self.0
    }
}

impl WasmSharedMemory for SharedMemory {
    fn read(&self, offset: u32, bytes: &mut [u8]) {
        range(&self.0, offset, bytes.len()).copy_to(bytes);
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
