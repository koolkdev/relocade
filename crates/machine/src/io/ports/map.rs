//! Owns port registrations and publishes them directly to the CPU's Wasm tables.

use super::{DevicePorts, PortRead, PortWrite};
use crate::io::State;
use crate::{runtime::Backend, Device, IoContext};
use relocade_wasm_runtime::{FunctionRef, Import, Result, Table, WasmBackend};
use relocade_x86::abi;
use std::{
    any::Any,
    cell::{Cell, OnceCell, RefCell},
    collections::HashMap,
    ops::RangeInclusive,
    rc::{Rc, Weak},
};

struct Entry<H> {
    owner: Rc<()>,
    handler: H,
}

struct PortMap<H> {
    entries: RefCell<Vec<Option<Entry<H>>>>,
    count: Cell<usize>,
}

impl<H> Default for PortMap<H> {
    fn default() -> Self {
        Self {
            entries: RefCell::default(),
            count: Cell::new(0),
        }
    }
}

impl<H: Clone> PortMap<H> {
    fn set(
        &self,
        device: &DevicePorts,
        ports: RangeInclusive<u16>,
        handler: Option<&H>,
        mut set_slot: impl FnMut(u16),
    ) {
        let mut entries = self.entries.borrow_mut();
        if entries.is_empty() {
            entries.resize_with(65536, || None);
        }
        assert!(
            ports.clone().all(|port| entries[usize::from(port)]
                .as_ref()
                .is_none_or(|entry| Rc::ptr_eq(&entry.owner, &device.identity))),
            "port belongs to another device"
        );
        let mut replaced = Vec::new();
        for port in ports {
            set_slot(port);
            let next = handler.map(|handler| Entry {
                owner: device.identity.clone(),
                handler: handler.clone(),
            });
            let old = std::mem::replace(&mut entries[usize::from(port)], next);
            self.count.set(
                self.count.get() + usize::from(handler.is_some()) - usize::from(old.is_some()),
            );
            replaced.push(old);
        }
        // Complete the entire range and release the map borrow before dropping
        // user captures.
        drop(entries);
        drop(replaced);
    }

    fn clear(&self, device: &DevicePorts, mut set_slot: impl FnMut(u16)) -> Vec<Entry<H>> {
        let mut entries = self.entries.borrow_mut();
        let mut removed = Vec::new();
        for (port, entry) in entries.iter_mut().enumerate() {
            if entry
                .as_ref()
                .is_some_and(|entry| Rc::ptr_eq(&entry.owner, &device.identity))
            {
                set_slot(port as u16);
                removed.push(entry.take().unwrap());
            }
        }
        self.count.set(self.count.get() - removed.len());
        removed
    }

    fn handler(&self, port: u16) -> Option<H> {
        self.entries
            .borrow()
            .get(usize::from(port))
            .and_then(Option::as_ref)
            .map(|entry| entry.handler.clone())
    }

    fn populate(&self, mut set_slot: impl FnMut(u16, &H)) {
        for (port, entry) in self.entries.borrow().iter().enumerate() {
            if let Some(entry) = entry {
                set_slot(port as u16, &entry.handler);
            }
        }
    }
}

struct Tables {
    reads: Table<Backend, abi::PortReadSignature>,
    writes: Table<Backend, abi::PortWriteSignature>,
    unhandled_read: FunctionRef<Backend, abi::PortReadSignature>,
    unhandled_write: FunctionRef<Backend, abi::PortWriteSignature>,
}

struct DeviceEntry {
    // A Weak<RefCell<D>> pins the allocation address without retaining D.
    _allocation: Box<dyn Any>,
    ports: DevicePorts,
}

#[derive(Default)]
pub(crate) struct Ports {
    devices: RefCell<HashMap<*const (), DeviceEntry>>,
    reads: PortMap<PortRead>,
    writes: PortMap<PortWrite>,
    tables: OnceCell<Tables>,
}

impl Ports {
    pub(in crate::io) fn device<D: Device + ?Sized + 'static>(
        &self,
        bus: &Weak<State>,
        device: &Rc<RefCell<D>>,
    ) -> DevicePorts {
        self.devices
            .borrow_mut()
            .entry(Rc::as_ptr(device).cast::<()>())
            .or_insert_with(|| DeviceEntry {
                _allocation: Box::new(Rc::downgrade(device)),
                ports: DevicePorts::new(bus),
            })
            .ports
            .clone()
    }

    pub(in crate::io) fn is_empty(&self) -> bool {
        self.reads.count.get() == 0 && self.writes.count.get() == 0
    }

    pub(crate) fn initialize(&self, backend: &mut Backend, bus: &Weak<State>) -> Result<()> {
        let read = FunctionRef::new(&mut backend.table_context(), |_, (_, _)| -1);
        let write = FunctionRef::new(&mut backend.table_context(), |_, (_, _, _)| ());
        let tables = Tables {
            reads: backend.create_table(abi::PORT_READS, Some(&read))?,
            writes: backend.create_table(abi::PORT_WRITES, Some(&write))?,
            unhandled_read: read,
            unhandled_write: write,
        };
        let mut context = IoContext::new(backend.table_context(), bus);
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

    pub(crate) fn imports<'a>(&'a self, backend: &Backend) -> Vec<Import<'a, Backend>> {
        let tables = self.tables.get().expect("initialized port tables");
        vec![
            backend.bind(abi::PORT_READS, &tables.reads),
            backend.bind(abi::PORT_WRITES, &tables.writes),
        ]
    }

    pub(super) fn set_read(
        &self,
        mut context: IoContext<'_>,
        device: &DevicePorts,
        ports: RangeInclusive<u16>,
        read: Option<&PortRead>,
    ) {
        if let Some(read) = read {
            read.0.check_owner(&context);
        }
        self.reads.set(device, ports, read, |port| {
            if let Some(tables) = self.tables.get() {
                let function =
                    read.map_or(&tables.unhandled_read, |read| read.0.function(&mut context));
                tables
                    .reads
                    .set(&mut context.runtime, port.into(), function)
                    .expect("port table slot");
            }
        });
    }

    pub(super) fn set_write(
        &self,
        mut context: IoContext<'_>,
        device: &DevicePorts,
        ports: RangeInclusive<u16>,
        write: Option<&PortWrite>,
    ) {
        if let Some(write) = write {
            write.0.check_owner(&context);
        }
        self.writes.set(device, ports, write, |port| {
            if let Some(tables) = self.tables.get() {
                let function = write.map_or(&tables.unhandled_write, |write| {
                    write.0.function(&mut context)
                });
                tables
                    .writes
                    .set(&mut context.runtime, port.into(), function)
                    .expect("port table slot");
            }
        });
    }

    pub(super) fn clear(&self, mut context: IoContext<'_>, device: &DevicePorts) {
        let reads = self.reads.clear(device, |port| {
            if let Some(tables) = self.tables.get() {
                tables
                    .reads
                    .set(&mut context.runtime, port.into(), &tables.unhandled_read)
                    .expect("port table slot");
            }
        });
        let writes = self.writes.clear(device, |port| {
            if let Some(tables) = self.tables.get() {
                tables
                    .writes
                    .set(&mut context.runtime, port.into(), &tables.unhandled_write)
                    .expect("port table slot");
            }
        });
        drop((reads, writes));
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
