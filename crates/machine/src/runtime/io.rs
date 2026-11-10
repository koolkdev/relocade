//! Port and MMIO callbacks access the device bus through their backend context.

use super::Backend;
use relocade_wasm_runtime::{FunctionRef, Import, WasmBackend, WasmContext};
use relocade_x86::abi;

pub(crate) fn imports(backend: &mut Backend) -> Vec<Import<'static, Backend>> {
    let mut context = backend.context();
    vec![
        FunctionRef::new(&mut context, |context, (port, bytes)| {
            context
                .state()
                .read_port_unchecked(port as u16, bytes as u8) as i32
        })
        .import(abi::READ_PORT),
        FunctionRef::new(&mut context, |context, (port, bytes, value)| {
            context
                .state()
                .write_port_unchecked(port as u16, bytes as u8, value as u32);
        })
        .import(abi::WRITE_PORT),
        FunctionRef::new(&mut context, |context, (address, bytes)| {
            context.state().read_mmio(address as u32, bytes as u8) as i64
        })
        .import(abi::READ_MMIO),
        FunctionRef::new(&mut context, |context, (address, bytes, value)| {
            context
                .state()
                .write_mmio(address as u32, bytes as u8, value as u64);
        })
        .import(abi::WRITE_MMIO),
    ]
}
