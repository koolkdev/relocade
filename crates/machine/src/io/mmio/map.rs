use super::{MmioRead, MmioWrite};
use crate::{io::State, IoContext, MachineConfig, Result};
use relocade_wasm_runtime::{
    FunctionRef, Import, Memory, Table, WasmBackend, WasmMemoryAccess, WasmRuntime,
};
use relocade_x86::{abi, MmioHandlerId, PhysicalMemoryMap};
use std::{
    cell::{OnceCell, RefCell},
    ops::RangeInclusive,
    rc::Weak,
};

struct Mapping {
    addresses: RangeInclusive<u32>,
    read: MmioRead,
    write: MmioWrite,
}

struct Tables {
    reads: Table<WasmRuntime, abi::MmioReadSignature>,
    writes: Table<WasmRuntime, abi::MmioWriteSignature>,
    physical: PhysicalPages,
}

struct PhysicalPages {
    memory: Memory,
    config: MachineConfig,
}

impl PhysicalPages {
    fn publish(
        &self,
        context: &mut IoContext<'_>,
        addresses: &RangeInclusive<u32>,
        slot: MmioHandlerId,
    ) {
        let page_bytes = PhysicalMemoryMap::PAGE_BYTES;
        let pages = *addresses.start() / page_bytes..=*addresses.end() / page_bytes;
        let mut memory = context.runtime.memory(&self.memory);
        for page in pages {
            let first = page * page_bytes;
            let handler = handler_for_page(addresses, first, slot);
            if let Some((offset, entry)) = self.config.memory_map().mmio_page_entry(first, handler)
            {
                memory.write(offset, &entry);
            }
        }
    }
}

/// A binding that covers only part of a page shares the generic range resolver.
fn handler_for_page(
    addresses: &RangeInclusive<u32>,
    first: u32,
    slot: MmioHandlerId,
) -> MmioHandlerId {
    let last = first | (PhysicalMemoryMap::PAGE_BYTES - 1);
    if addresses.contains(&first) && addresses.contains(&last) {
        slot
    } else {
        MmioHandlerId::GENERIC
    }
}

/// Slot zero resolves mixed pages. Every registered range owns one further slot;
/// complete physical pages point directly to that range's prepared operations.
#[derive(Default)]
pub(crate) struct Mmio {
    mappings: RefCell<Vec<Mapping>>,
    tables: OnceCell<Tables>,
}

impl Mmio {
    pub(crate) fn initialize(
        &self,
        wasm: &mut WasmRuntime,
        bus: &Weak<State>,
        memory: &Memory,
        config: &MachineConfig,
    ) -> Result<()> {
        let read_bus = bus.clone();
        let read = FunctionRef::new(&mut wasm.context(), move |runtime, (address, bytes)| {
            let bus = read_bus.upgrade().expect("live I/O bus");
            bus.mmio.read(
                IoContext::new(runtime, &read_bus),
                address as u32,
                bytes as u8,
            ) as i64
        });
        let write_bus = bus.clone();
        let write = FunctionRef::new(
            &mut wasm.context(),
            move |runtime, (address, bytes, value)| {
                let bus = write_bus.upgrade().expect("live I/O bus");
                bus.mmio.write(
                    IoContext::new(runtime, &write_bus),
                    address as u32,
                    bytes as u8,
                    value as u64,
                );
            },
        );
        let tables = Tables {
            reads: wasm.create_table(abi::MMIO_READS, Some(&read))?,
            writes: wasm.create_table(abi::MMIO_WRITES, Some(&write))?,
            physical: PhysicalPages {
                memory: memory.clone(),
                config: config.clone(),
            },
        };
        let mut context = IoContext::new(wasm.context(), bus);
        // Handler indices belong to this bus. Explicit reservations start at
        // the generic handler until a connected device supplies its operations.
        for (_, offset) in config.memory_map().mmio_handler_offsets() {
            context
                .runtime
                .memory(memory)
                .write(offset, &MmioHandlerId::GENERIC.index().to_le_bytes());
        }
        for (index, mapping) in self.mappings.borrow().iter().enumerate() {
            let slot = MmioHandlerId::new(index as u32 + 1);
            Self::install(
                &tables,
                &mut context,
                slot.index(),
                &mapping.read,
                &mapping.write,
            );
            tables
                .physical
                .publish(&mut context, &mapping.addresses, slot);
        }
        self.tables.set(tables).unwrap_or_else(|_| unreachable!());
        Ok(())
    }

    pub(crate) fn imports<'a>(&'a self, wasm: &WasmRuntime) -> [Import<'a, WasmRuntime>; 2] {
        let tables = self.tables.get().expect("initialized MMIO tables");
        [
            wasm.bind(abi::MMIO_READS, &tables.reads),
            wasm.bind(abi::MMIO_WRITES, &tables.writes),
        ]
    }

    pub(in crate::io) fn bind(
        &self,
        mut context: IoContext<'_>,
        addresses: RangeInclusive<u32>,
        read: &MmioRead,
        write: &MmioWrite,
    ) -> MmioHandlerId {
        read.0.function(&mut context);
        write.0.function(&mut context);
        let index = {
            let mut mappings = self.mappings.borrow_mut();
            mappings.push(Mapping {
                addresses: addresses.clone(),
                read: read.clone(),
                write: write.clone(),
            });
            u32::try_from(mappings.len()).expect("MMIO handler index")
        };
        let slot = MmioHandlerId::new(index);
        if let Some(tables) = self.tables.get() {
            Self::install(tables, &mut context, index, read, write);
            // Publish the mapping only after both operations are callable.
            tables.physical.publish(&mut context, &addresses, slot);
        }
        slot
    }

    fn install(
        tables: &Tables,
        context: &mut IoContext<'_>,
        index: u32,
        read: &MmioRead,
        write: &MmioWrite,
    ) {
        tables
            .reads
            .ensure_size(&mut context.runtime, index + 1, None)
            .expect("grow MMIO read table");
        tables
            .writes
            .ensure_size(&mut context.runtime, index + 1, None)
            .expect("grow MMIO write table");
        let function = read.0.function(context);
        tables
            .reads
            .set(&mut context.runtime, index, function)
            .expect("MMIO read slot");
        let function = write.0.function(context);
        tables
            .writes
            .set(&mut context.runtime, index, function)
            .expect("MMIO write slot");
    }

    pub(in crate::io) fn set_read(
        &self,
        mut context: IoContext<'_>,
        handler: MmioHandlerId,
        read: &MmioRead,
    ) {
        let function = read.0.function(&mut context);
        if let Some(tables) = self.tables.get() {
            tables
                .reads
                .set(&mut context.runtime, handler.index(), function)
                .expect("MMIO read slot");
        }
        let replaced = std::mem::replace(
            &mut self.mappings.borrow_mut()[handler.index() as usize - 1].read,
            read.clone(),
        );
        drop(replaced);
    }

    pub(in crate::io) fn set_write(
        &self,
        mut context: IoContext<'_>,
        handler: MmioHandlerId,
        write: &MmioWrite,
    ) {
        let function = write.0.function(&mut context);
        if let Some(tables) = self.tables.get() {
            tables
                .writes
                .set(&mut context.runtime, handler.index(), function)
                .expect("MMIO write slot");
        }
        let replaced = std::mem::replace(
            &mut self.mappings.borrow_mut()[handler.index() as usize - 1].write,
            write.clone(),
        );
        drop(replaced);
    }

    pub(crate) fn page_handler(&self, address: u32) -> Option<MmioHandlerId> {
        let first = address & !(PhysicalMemoryMap::PAGE_BYTES - 1);
        let last = first | (PhysicalMemoryMap::PAGE_BYTES - 1);
        self.mappings
            .borrow()
            .iter()
            .enumerate()
            .find(|(_, mapping)| {
                *mapping.addresses.start() <= last && first <= *mapping.addresses.end()
            })
            .map(|(index, mapping)| {
                handler_for_page(
                    &mapping.addresses,
                    first,
                    MmioHandlerId::new(index as u32 + 1),
                )
            })
    }

    fn span(&self, address: u32, bytes: u8) -> (Option<usize>, u8) {
        let mut count = u64::from(bytes);
        let mut owner = None;
        for (index, mapping) in self.mappings.borrow().iter().enumerate() {
            if mapping.addresses.contains(&address) {
                count = count.min(u64::from(*mapping.addresses.end()) - u64::from(address) + 1);
                owner = Some(index);
                break;
            }
            if *mapping.addresses.start() > address {
                count = count.min(u64::from(*mapping.addresses.start()) - u64::from(address));
            }
        }
        let in_page = PhysicalMemoryMap::PAGE_BYTES - address % PhysicalMemoryMap::PAGE_BYTES;
        if u64::from(in_page) < count
            && self.page_handler(address) != self.page_handler(address + in_page)
        {
            count = u64::from(in_page);
        }
        (owner, count as u8)
    }

    pub(in crate::io) fn read(&self, mut context: IoContext<'_>, address: u32, bytes: u8) -> u64 {
        let mut result = 0;
        let mut done = 0;
        while done < bytes {
            let current = address + u32::from(done);
            let (owner, count) = self.span(current, bytes - done);
            let read = owner.map(|index| self.mappings.borrow()[index].read.clone());
            let value = read.map_or(u64::MAX, |read| {
                (read.0.callback)(context.reborrow(), current, count)
            });
            result |= (value & mask(count)) << (done * 8);
            done += count;
        }
        result
    }

    pub(in crate::io) fn write(
        &self,
        mut context: IoContext<'_>,
        address: u32,
        bytes: u8,
        value: u64,
    ) {
        let mut done = 0;
        while done < bytes {
            let current = address + u32::from(done);
            let (owner, count) = self.span(current, bytes - done);
            let write = owner.map(|index| self.mappings.borrow()[index].write.clone());
            if let Some(write) = write {
                (write.0.callback)(
                    context.reborrow(),
                    current,
                    count,
                    (value >> (done * 8)) & mask(count),
                );
            }
            done += count;
        }
    }
}

fn mask(bytes: u8) -> u64 {
    u64::MAX >> ((8 - bytes) * 8)
}
