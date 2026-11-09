//! Shared port tables and MMIO callbacks for every module linked to one CPU.

mod ports;

use super::Backend;
use crate::IoBus;
pub use ports::PortMappings;
use ports::PortTables;
use relocade_wasm_runtime::{Import, Result, Table, WasmBackend, WasmContext};
use relocade_x86::abi;

type Context<'a> = <Backend as WasmBackend>::Context<'a>;

pub(super) struct State {
    io: IoBus,
    ports: Option<PortTables>,
}

impl State {
    pub(super) fn new(io: IoBus) -> Self {
        Self { io, ports: None }
    }
}

pub(super) struct IoBindings {
    reads: Table<Backend, abi::PortReadSignature>,
    writes: Table<Backend, abi::PortWriteSignature>,
    mmio: Vec<Import<'static, Backend>>,
}

impl IoBindings {
    pub(super) fn new(backend: &mut Backend) -> Result<Self> {
        let ports = PortTables::new(backend)?;
        let reads = ports.reads.clone();
        let writes = ports.writes.clone();
        backend.state_mut().ports = Some(ports);
        Ok(Self {
            reads,
            writes,
            mmio: vec![
                backend.bind_host(
                    abi::READ_MMIO,
                    Box::new(|context, address, bytes| {
                        let value = context.state().io.read_mmio(address as u32, bytes as u8);
                        value as i64
                    }),
                ),
                backend.bind_host(
                    abi::WRITE_MMIO,
                    Box::new(|context, address, bytes, value| {
                        context
                            .state()
                            .io
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
