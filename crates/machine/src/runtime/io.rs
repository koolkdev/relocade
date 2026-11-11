//! Shared port tables and MMIO callbacks for every module linked to one CPU.

use super::Backend;
use crate::{IoBus, IoContext};
use relocade_compiler::{I32, I64};
use relocade_wasm_runtime::{FunctionRef, Import, Result, WasmBackend};
use relocade_x86::abi;

pub(super) struct IoBindings {
    bus: IoBus,
    mmio: Vec<Import<'static, Backend>>,
}

impl IoBindings {
    pub(super) fn new(backend: &mut Backend, io: &IoBus) -> Result<Self> {
        io.port_map().initialize(backend, &io.downgrade())?;
        let bus = io.downgrade();
        let read_mmio = FunctionRef::<Backend, fn(I32, I32) -> I64>::new(
            &mut backend.table_context(),
            move |context, (address, bytes)| {
                bus.upgrade().expect("live I/O bus").read_mmio(
                    IoContext::new(context, &bus),
                    address as u32,
                    bytes as u8,
                ) as i64
            },
        );
        let bus = io.downgrade();
        let write_mmio = FunctionRef::<Backend, fn(I32, I32, I64)>::new(
            &mut backend.table_context(),
            move |context, (address, bytes, value)| {
                bus.upgrade().expect("live I/O bus").write_mmio(
                    IoContext::new(context, &bus),
                    address as u32,
                    bytes as u8,
                    value as u64,
                );
            },
        );
        Ok(Self {
            bus: io.clone(),
            mmio: vec![
                read_mmio.import(abi::READ_MMIO),
                write_mmio.import(abi::WRITE_MMIO),
            ],
        })
    }

    pub(super) fn imports<'a>(&'a self, backend: &Backend) -> Vec<Import<'a, Backend>> {
        let mut imports = self.bus.port_map().imports(backend);
        imports.extend(self.mmio.iter().cloned());
        imports
    }
}
