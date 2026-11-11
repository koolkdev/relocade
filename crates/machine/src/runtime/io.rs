//! Port and MMIO callbacks capture the CPU's fixed device routing.

use super::WasmRuntime;
use crate::IoBus;
use relocade_wasm_runtime::{FunctionRef, Import, WasmBackend};
use relocade_x86::abi;

pub(crate) fn imports(wasm: &mut WasmRuntime, io: &IoBus) -> Vec<Import<'static, WasmRuntime>> {
    let mut context = wasm.context();
    let bus = io.downgrade();
    let read = FunctionRef::new(&mut context, move |_, (port, bytes)| {
        bus.upgrade()
            .expect("live I/O bus")
            .read_port_unchecked(port as u16, bytes as u8) as i32
    });
    let bus = io.downgrade();
    let write = FunctionRef::new(&mut context, move |_, (port, bytes, value)| {
        bus.upgrade().expect("live I/O bus").write_port_unchecked(
            port as u16,
            bytes as u8,
            value as u32,
        );
    });
    let bus = io.downgrade();
    let read_mmio = FunctionRef::new(&mut context, move |_, (address, bytes)| {
        bus.upgrade()
            .expect("live I/O bus")
            .read_mmio(address as u32, bytes as u8) as i64
    });
    let bus = io.downgrade();
    let write_mmio = FunctionRef::new(&mut context, move |_, (address, bytes, value)| {
        bus.upgrade()
            .expect("live I/O bus")
            .write_mmio(address as u32, bytes as u8, value as u64);
    });
    vec![
        read.import(abi::READ_PORT),
        write.import(abi::WRITE_PORT),
        read_mmio.import(abi::READ_MMIO),
        write_mmio.import(abi::WRITE_MMIO),
    ]
}
