//! Shared port tables and MMIO callbacks for every module linked to one CPU.

use super::Backend;
use relocade_wasm_runtime::{Import, Result, Table, WasmBackend, WasmContext};
use relocade_x86::abi;

pub(super) struct IoBindings {
    reads: Table<Backend, abi::PortReadSignature>,
    writes: Table<Backend, abi::PortWriteSignature>,
    mmio: Vec<Import<'static, Backend>>,
}

impl IoBindings {
    pub(super) fn new(backend: &mut Backend) -> Result<Self> {
        let read =
            backend.host_function::<abi::PortReadSignature>(Box::new(|context, port, bytes| {
                context.state().read_port(port as u16, bytes as u8) as i32
            }));
        let write = backend.host_function::<abi::PortWriteSignature>(Box::new(
            |context, port, bytes, value| {
                context
                    .state()
                    .write_port(port as u16, bytes as u8, value as u32);
            },
        ));
        Ok(Self {
            reads: backend.create_table(abi::PORT_READS, Some(&read))?,
            writes: backend.create_table(abi::PORT_WRITES, Some(&write))?,
            mmio: vec![
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
