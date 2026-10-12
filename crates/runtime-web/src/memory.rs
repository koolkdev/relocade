use crate::{js_error, set_property};
use js_sys::{Function, Object, Reflect, Uint8Array, WebAssembly::Memory};
use wasm86_runtime::{backend::GuestMemory, Result};
use wasm86_x86::PhysicalMemoryMap;
use wasm_bindgen::{JsCast, JsValue};

pub(crate) fn new(maximum_pages: u32) -> Result<Memory> {
    let descriptor = Object::new();
    set_property(&descriptor, "initial", &1.into());
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
    pub physical: Memory,
}

impl MemoryAccess {
    pub fn grow(&mut self, bytes: u64) -> Result<()> {
        let pages = bytes.div_ceil(65536);
        let current = self.byte_len() / 65536;
        if pages > current {
            // js-sys's direct Memory::grow binding does not catch allocation
            // failures. Calling its function through call1 preserves the error.
            let grow: Function = Reflect::get(&self.guest, &"grow".into())
                .expect("WebAssembly.Memory has grow")
                .unchecked_into();
            grow.call1(&self.guest, &JsValue::from_f64((pages - current) as f64))
                .map_err(|error| js_error("grow guest memory", error))?;
        }
        Ok(())
    }
}

impl GuestMemory for MemoryAccess {
    fn byte_len(&self) -> u64 {
        // ArrayBuffer's JS byteLength can be 2^32; reading it as a wasm32 usize
        // would truncate the capacity of a full 4 GiB memory to zero.
        Reflect::get(&self.guest.buffer(), &"byteLength".into())
            .expect("memory buffer has a byte length")
            .as_f64()
            .expect("memory buffer byte length is numeric") as u64
    }

    fn read(&self, offset: u32, bytes: &mut [u8]) {
        read(&self.guest, offset, bytes);
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) {
        write(&self.guest, offset, bytes);
    }

    fn publish_map(&mut self, bytes: &[u8; PhysicalMemoryMap::BYTE_LEN]) {
        write(&self.physical, 0, bytes);
    }
}
