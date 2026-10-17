use crate::{js_error, set_property};
use js_sys::{Object, Reflect, Uint8Array, WebAssembly::Memory};
use wasm86_runtime::{backend::GuestMemory, Result};

pub(crate) fn new(initial_pages: u32, maximum_pages: u32) -> Result<Memory> {
    let descriptor = Object::new();
    set_property(&descriptor, "initial", &initial_pages.into());
    set_property(&descriptor, "maximum", &maximum_pages.into());
    Memory::new(&descriptor).map_err(|error| js_error("allocate Wasm memory", error))
}

// Each transfer takes a fresh view: memory.grow detaches earlier ArrayBuffers.
// Views never escape into user callbacks or survive another Wasm entry.
pub(crate) fn read(memory: &Memory, offset: u32, bytes: &mut [u8]) {
    Uint8Array::new_with_byte_offset_and_length(&memory.buffer(), offset, bytes.len() as u32)
        .copy_to(bytes);
}

pub(crate) fn write(memory: &Memory, offset: u32, bytes: &[u8]) {
    Uint8Array::new_with_byte_offset_and_length(&memory.buffer(), offset, bytes.len() as u32)
        .copy_from(bytes);
}

#[derive(Clone)]
pub(crate) struct MemoryAccess {
    pub guest: Memory,
    pub mapping: Memory,
}

fn byte_len(memory: &Memory) -> u64 {
    // JS byteLength can be 2^32; a wasm32 usize would truncate that capacity.
    Reflect::get(&memory.buffer(), &"byteLength".into())
        .expect("memory buffer has a byte length")
        .as_f64()
        .expect("memory buffer byte length is numeric") as u64
}

impl GuestMemory for MemoryAccess {
    fn byte_len(&self) -> u64 {
        byte_len(&self.guest)
    }

    fn read(&self, offset: u32, bytes: &mut [u8]) {
        read(&self.guest, offset, bytes);
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) {
        write(&self.guest, offset, bytes);
    }

    fn publish_map(&mut self, bytes: &[u8]) {
        write(&self.mapping, 0, bytes);
    }
}
