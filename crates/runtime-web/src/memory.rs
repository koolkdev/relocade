use crate::{js_error, set_property};
use js_sys::{Function, Object, Reflect, Uint8Array, WebAssembly::Memory};
use wasm86_runtime::{backend::GuestMemory, Result};
use wasm_bindgen::{JsCast, JsValue};

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

/// Grows a backend-owned memory without retaining a view across detachment.
pub(crate) fn grow(memory: &Memory, bytes: u64) -> Result<()> {
    let pages = bytes.div_ceil(65536);
    let current = byte_len(memory) / 65536;
    if pages > current {
        // js-sys's direct binding does not catch allocation failures.
        let grow: Function = Reflect::get(memory, &"grow".into())
            .expect("WebAssembly.Memory has grow")
            .unchecked_into();
        grow.call1(memory, &JsValue::from_f64((pages - current) as f64))
            .map_err(|error| js_error("grow Wasm memory", error))?;
    }
    Ok(())
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
