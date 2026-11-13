//! Shared MMIO tables initially retain the bus's generic range resolver.

use super::WasmRuntime;
use crate::{IoBus, IoContext, Result};
use relocade_wasm_runtime::{FunctionRef, Import, Table, WasmBackend};
use relocade_x86::abi;

pub(super) struct MmioTables {
    reads: Table<WasmRuntime, abi::MmioReadSignature>,
    writes: Table<WasmRuntime, abi::MmioWriteSignature>,
}

impl MmioTables {
    pub(super) fn new(wasm: &mut WasmRuntime, io: &IoBus) -> Result<Self> {
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
        Ok(Self {
            reads: wasm.create_table(abi::MMIO_READS, Some(&read_mmio))?,
            writes: wasm.create_table(abi::MMIO_WRITES, Some(&write_mmio))?,
        })
    }

    pub(super) fn imports<'a>(&'a self, wasm: &WasmRuntime) -> [Import<'a, WasmRuntime>; 2] {
        [
            wasm.bind(abi::MMIO_READS, &self.reads),
            wasm.bind(abi::MMIO_WRITES, &self.writes),
        ]
    }
}
