//! Port and MMIO callbacks access the device bus through their backend context.

use super::Backend;
use relocade_wasm_runtime::{Import, WasmBackend, WasmContext};
use relocade_x86::abi;

pub(crate) fn imports(backend: &mut Backend) -> Vec<Import<'static, Backend>> {
    vec![
        backend.bind_host(
            abi::READ_PORT,
            Box::new(|context, port, bytes| {
                context.state().read_port(port as u16, bytes as u8) as i32
            }),
        ),
        backend.bind_host(
            abi::WRITE_PORT,
            Box::new(|context, port, bytes, value| {
                context
                    .state()
                    .write_port(port as u16, bytes as u8, value as u32);
            }),
        ),
        backend.bind_host(
            abi::READ_MMIO,
            Box::new(|context, address, bytes| {
                context.state().read_mmio(address as u32, bytes as u8) as i64
            }),
        ),
        backend.bind_host(
            abi::WRITE_MMIO,
            Box::new(|context, address, bytes, value| {
                context
                    .state()
                    .write_mmio(address as u32, bytes as u8, value as u64);
            }),
        ),
    ]
}
