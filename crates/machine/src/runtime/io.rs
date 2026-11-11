//! Shared port tables and MMIO callbacks for every module linked to one CPU.

use super::Backend;
use crate::IoBus;
use relocade_compiler::{I32, I64};
use relocade_wasm_runtime::{FunctionRef, Import, Result, Table, WasmBackend};
use relocade_x86::abi;

pub(super) struct IoBindings {
    reads: Table<Backend, abi::PortReadSignature>,
    writes: Table<Backend, abi::PortWriteSignature>,
    mmio: Vec<Import<'static, Backend>>,
}

impl IoBindings {
    pub(super) fn new(backend: &mut Backend, io: &IoBus) -> Result<Self> {
        let bus = io.downgrade();
        let read = FunctionRef::<Backend, abi::PortReadSignature>::new(
            &mut backend.table_context(),
            move |_, (port, bytes)| {
                bus.upgrade()
                    .expect("live I/O bus")
                    .read_port_unchecked(port as u16, bytes as u8) as i32
            },
        );
        let bus = io.downgrade();
        let write = FunctionRef::<Backend, abi::PortWriteSignature>::new(
            &mut backend.table_context(),
            move |_, (port, bytes, value)| {
                bus.upgrade().expect("live I/O bus").write_port_unchecked(
                    port as u16,
                    bytes as u8,
                    value as u32,
                );
            },
        );
        let bus = io.downgrade();
        let read_mmio = FunctionRef::<Backend, fn(I32, I32) -> I64>::new(
            &mut backend.table_context(),
            move |_, (address, bytes)| {
                bus.upgrade()
                    .expect("live I/O bus")
                    .read_mmio(address as u32, bytes as u8) as i64
            },
        );
        let bus = io.downgrade();
        let write_mmio = FunctionRef::<Backend, fn(I32, I32, I64)>::new(
            &mut backend.table_context(),
            move |_, (address, bytes, value)| {
                bus.upgrade().expect("live I/O bus").write_mmio(
                    address as u32,
                    bytes as u8,
                    value as u64,
                );
            },
        );
        Ok(Self {
            reads: backend.create_table(abi::PORT_READS, Some(&read))?,
            writes: backend.create_table(abi::PORT_WRITES, Some(&write))?,
            mmio: vec![
                read_mmio.import(abi::READ_MMIO),
                write_mmio.import(abi::WRITE_MMIO),
            ],
        })
    }

    pub(super) fn imports<'a>(&'a self, backend: &Backend) -> Vec<Import<'a, Backend>> {
        let mut imports = vec![
            backend.bind(abi::PORT_READS, &self.reads),
            backend.bind(abi::PORT_WRITES, &self.writes),
        ];
        imports.extend(self.mmio.iter().cloned());
        imports
    }
}
