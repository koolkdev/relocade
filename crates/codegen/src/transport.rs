//! Single-caller buffer ABI for a generator instance owned by one Wasm worker.
//! JavaScript writes input only between calls and copies output before the next
//! request. No caller-provided pointer is dereferenced by Rust.

use crate::Request;
use std::cell::RefCell;

#[derive(Default)]
struct Buffers {
    input: Vec<u8>,
    wasm: Vec<u8>,
    metadata: Vec<u8>,
}
thread_local! { static BUFFERS: RefCell<Buffers> = RefCell::default(); }

/// Returns zero for oversized input; otherwise the writable request buffer.
#[no_mangle]
pub extern "C" fn request_buffer(length: u32) -> *mut u8 {
    if length > 65536 {
        return std::ptr::null_mut();
    }
    BUFFERS.with_borrow_mut(|buffers| {
        buffers.input.resize(length as usize, 0);
        buffers.input.as_mut_ptr()
    })
}

#[no_mangle]
pub extern "C" fn generate() {
    BUFFERS.with_borrow_mut(|buffers| {
        buffers.wasm.clear();
        let result = serde_json::from_slice::<Request>(&buffers.input)
            .map_err(|error| error.to_string())
            .and_then(|request| request.generate());
        let metadata = match result {
            Ok(module) => {
                buffers.wasm = module.bytes;
                serde_json::json!({ "entry": module.entry })
            }
            Err(error) => serde_json::json!({ "error": error }),
        };
        buffers.metadata = serde_json::to_vec(&metadata).expect("metadata is serializable");
    });
}

#[no_mangle]
pub extern "C" fn wasm_pointer() -> *const u8 {
    BUFFERS.with_borrow(|buffers| buffers.wasm.as_ptr())
}
#[no_mangle]
pub extern "C" fn wasm_length() -> u32 {
    BUFFERS.with_borrow(|buffers| buffers.wasm.len() as u32)
}
#[no_mangle]
pub extern "C" fn metadata_pointer() -> *const u8 {
    BUFFERS.with_borrow(|buffers| buffers.metadata.as_ptr())
}
#[no_mangle]
pub extern "C" fn metadata_length() -> u32 {
    BUFFERS.with_borrow(|buffers| buffers.metadata.len() as u32)
}
