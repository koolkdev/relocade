//! MMIO callbacks use the current engine scope and the CPU's fixed bus.

use super::WasmRuntime;
use crate::{IoBus, IoContext};
use relocade_wasm_runtime::{FunctionRef, Import, WasmBackend};
use relocade_x86::abi;

pub(super) fn mmio_imports(
    wasm: &mut WasmRuntime,
    io: &IoBus,
) -> Vec<Import<'static, WasmRuntime>> {
    let mut context = wasm.context();
    let bus = io.downgrade();
    let read_mmio = FunctionRef::new(&mut context, move |context, (address, bytes)| {
        bus.upgrade().expect("live I/O bus").read_mmio(
            IoContext::new(context, &bus),
            address as u32,
            bytes as u8,
        ) as i64
    });
    let bus = io.downgrade();
    let write_mmio = FunctionRef::new(&mut context, move |context, (address, bytes, value)| {
        bus.upgrade().expect("live I/O bus").write_mmio(
            IoContext::new(context, &bus),
            address as u32,
            bytes as u8,
            value as u64,
        );
    });
    vec![
        read_mmio.import(abi::READ_MMIO),
        write_mmio.import(abi::WRITE_MMIO),
    ]
}
