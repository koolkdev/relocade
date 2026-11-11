use super::{PortRead, PortWrite};
use crate::{io::State, IoContext};
use relocade_wasm_runtime::{FunctionRef, Import, Result, Table, WasmBackend, WasmRuntime};
use relocade_x86::abi;
use std::{
    cell::{OnceCell, RefCell},
    ops::RangeInclusive,
    rc::Weak,
};

struct PortMap<H> {
    entries: RefCell<Vec<Option<H>>>,
}

impl<H> Default for PortMap<H> {
    fn default() -> Self {
        Self {
            entries: RefCell::default(),
        }
    }
}

impl<H: Clone> PortMap<H> {
    fn set(&self, ports: RangeInclusive<u16>, handler: &H, mut publish: impl FnMut(u16)) {
        let mut entries = self.entries.borrow_mut();
        if entries.is_empty() {
            entries.resize_with(65536, || None);
        }
        let mut replaced = Vec::new();
        for port in ports {
            publish(port);
            replaced.push(entries[usize::from(port)].replace(handler.clone()));
        }
        // Captures may own application resources; release them outside the map borrow.
        drop(entries);
        drop(replaced);
    }

    fn handler(&self, port: u16) -> Option<H> {
        self.entries
            .borrow()
            .get(usize::from(port))
            .and_then(Clone::clone)
    }

    fn populate(&self, mut publish: impl FnMut(u16, &H)) {
        for (port, entry) in self.entries.borrow().iter().enumerate() {
            if let Some(handler) = entry {
                publish(port as u16, handler);
            }
        }
    }
}

struct Tables {
    reads: Table<WasmRuntime, abi::PortReadSignature>,
    writes: Table<WasmRuntime, abi::PortWriteSignature>,
}

#[derive(Default)]
pub(crate) struct Ports {
    reads: PortMap<PortRead>,
    writes: PortMap<PortWrite>,
    tables: OnceCell<Tables>,
}

impl Ports {
    pub(crate) fn initialize(&self, wasm: &mut WasmRuntime, bus: &Weak<State>) -> Result<()> {
        let read = FunctionRef::new(&mut wasm.context(), |_, (_, _)| -1);
        let write = FunctionRef::new(&mut wasm.context(), |_, (_, _, _)| ());
        let tables = Tables {
            reads: wasm.create_table(abi::PORT_READS, Some(&read))?,
            writes: wasm.create_table(abi::PORT_WRITES, Some(&write))?,
        };
        let mut context = IoContext::new(wasm.context(), bus);
        self.reads.populate(|port, read| {
            let function = read.0.function(&mut context);
            tables
                .reads
                .set(&mut context.runtime, port.into(), function)
                .expect("port table slot");
        });
        self.writes.populate(|port, write| {
            let function = write.0.function(&mut context);
            tables
                .writes
                .set(&mut context.runtime, port.into(), function)
                .expect("port table slot");
        });
        self.tables.set(tables).unwrap_or_else(|_| unreachable!());
        Ok(())
    }

    pub(crate) fn imports<'a>(&'a self, wasm: &WasmRuntime) -> Vec<Import<'a, WasmRuntime>> {
        let tables = self.tables.get().expect("initialized port tables");
        vec![
            wasm.bind(abi::PORT_READS, &tables.reads),
            wasm.bind(abi::PORT_WRITES, &tables.writes),
        ]
    }

    pub(in crate::io) fn set_read(
        &self,
        mut context: IoContext<'_>,
        ports: RangeInclusive<u16>,
        read: &PortRead,
    ) {
        self.reads.set(ports, read, |port| {
            if let Some(tables) = self.tables.get() {
                let function = read.0.function(&mut context);
                tables
                    .reads
                    .set(&mut context.runtime, port.into(), function)
                    .expect("port table slot");
            }
        });
    }

    pub(in crate::io) fn set_write(
        &self,
        mut context: IoContext<'_>,
        ports: RangeInclusive<u16>,
        write: &PortWrite,
    ) {
        self.writes.set(ports, write, |port| {
            if let Some(tables) = self.tables.get() {
                let function = write.0.function(&mut context);
                tables
                    .writes
                    .set(&mut context.runtime, port.into(), function)
                    .expect("port table slot");
            }
        });
    }

    pub(in crate::io) fn read(&self, context: IoContext<'_>, port: u16, bytes: u8) -> u32 {
        self.reads
            .handler(port)
            .map_or(u32::MAX, |read| (read.0.callback)(context, port, bytes))
    }

    pub(in crate::io) fn write(&self, context: IoContext<'_>, port: u16, bytes: u8, value: u32) {
        if let Some(write) = self.writes.handler(port) {
            (write.0.callback)(context, port, bytes, value);
        }
    }
}
