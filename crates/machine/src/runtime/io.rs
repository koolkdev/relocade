//! Port and MMIO bindings for the same device bus used by host memory views.

use super::Backend;
use crate::IoBus;
use relocade_wasm_runtime::{Import, WasmBackend};
use relocade_x86::abi;

pub(crate) fn imports(backend: &mut Backend, io: &IoBus) -> Vec<Import<'static, Backend>> {
    let read_ports = io.clone();
    let write_ports = io.clone();
    let read_mmio = io.clone();
    let write_mmio = io.clone();
    vec![
        backend.bind_host(
            abi::READ_PORT,
            Box::new(move |_, port, bytes| read_ports.read_port(port as u16, bytes as u8) as i32),
        ),
        backend.bind_host(
            abi::WRITE_PORT,
            Box::new(move |_, port, bytes, value| {
                write_ports.write_port(port as u16, bytes as u8, value as u32);
            }),
        ),
        backend.bind_host(
            abi::READ_MMIO,
            Box::new(move |_, address, bytes| {
                read_mmio.read_mmio(address as u32, bytes as u8) as i64
            }),
        ),
        backend.bind_host(
            abi::WRITE_MMIO,
            Box::new(move |_, address, bytes, value| {
                write_mmio.write_mmio(address as u32, bytes as u8, value as u64);
            }),
        ),
    ]
}
