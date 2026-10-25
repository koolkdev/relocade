//! Default port and MMIO bindings when the CPU has no connected devices.

use super::Backend;
use relocade_wasm_runtime::{Import, WasmBackend};
use relocade_x86::abi;

/// Reads return all ones for the requested width; writes are ignored.
pub(crate) fn unconnected_imports(backend: &mut Backend) -> Vec<Import<'static, Backend>> {
    vec![
        backend.bind_host(abi::READ_PORT, Box::new(|_, _, _| -1)),
        backend.bind_host(abi::WRITE_PORT, Box::new(|_, _, _, _| {})),
        backend.bind_host(abi::READ_MMIO, Box::new(|_, _, _| -1)),
        backend.bind_host(abi::WRITE_MMIO, Box::new(|_, _, _, _| {})),
    ]
}
