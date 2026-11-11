//! Shared port tables and MMIO callbacks for every module linked to one CPU.

use super::WasmRuntime;
use crate::{IoBus, IoContext};
use relocade_wasm_runtime::{FunctionRef, Import, Result, Table, WasmBackend};
use relocade_x86::abi;

pub(super) struct IoBindings {
    reads: Table<WasmRuntime, abi::PortReadSignature>,
    writes: Table<WasmRuntime, abi::PortWriteSignature>,
    mmio: Vec<Import<'static, WasmRuntime>>,
}

impl IoBindings {
    pub(super) fn new(wasm: &mut WasmRuntime, io: &IoBus) -> Result<Self> {
        let mut context = wasm.context();
        let bus = io.downgrade();
        let read = FunctionRef::new(&mut context, move |context, (port, bytes)| {
            bus.upgrade().expect("live I/O bus").read_port_unchecked(
                IoContext::new(context, &bus),
                port as u16,
                bytes as u8,
            ) as i32
        });
        let bus = io.downgrade();
        let write = FunctionRef::new(&mut context, move |context, (port, bytes, value)| {
            bus.upgrade().expect("live I/O bus").write_port_unchecked(
                IoContext::new(context, &bus),
                port as u16,
                bytes as u8,
                value as u32,
            );
        });
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
            reads: wasm.create_table(abi::PORT_READS, Some(&read))?,
            writes: wasm.create_table(abi::PORT_WRITES, Some(&write))?,
            mmio: vec![
                read_mmio.import(abi::READ_MMIO),
                write_mmio.import(abi::WRITE_MMIO),
            ],
        })
    }

    pub(super) fn imports<'a>(&'a self, wasm: &WasmRuntime) -> Vec<Import<'a, WasmRuntime>> {
        let mut imports = vec![
            wasm.bind(abi::PORT_READS, &self.reads),
            wasm.bind(abi::PORT_WRITES, &self.writes),
        ];
        imports.extend(self.mmio.iter().cloned());
        imports
    }
}
