//! Direct physical backing accesses and shared routing for MMIO and split spans.

use crate::memory::TransferType;
mod table;
mod transfer;

use relocade_compiler::{
    BlockBuilder, BuildError, Mem, MemoryInt, MemoryType, Program, TypedFunc, Val, I32,
};

use super::{Access, Intent};
use crate::alu::OperandUpdate;
use table::PhysicalTable;

pub(crate) struct PhysicalMemory {
    backing: Mem,
    table: PhysicalTable,
    reader: TypedFunc<fn(u32, u32) -> u64>,
    writer: TypedFunc<fn(u32, u32, u64)>,
}

impl PhysicalMemory {
    pub(super) fn declare(program: &mut Program) -> Self {
        let backing = program.import_memory(crate::abi::GUEST);
        let table = PhysicalTable::declare(program);
        let mmio_reader = program.import_table(crate::abi::MMIO_READS);
        let mmio_writer = program.import_table(crate::abi::MMIO_WRITES);
        Self {
            backing,
            table,
            reader: transfer::declare_reader(program, backing, table, mmio_reader),
            writer: transfer::declare_writer(program, backing, table, mmio_writer),
        }
    }

    pub(super) fn backing(&self) -> Mem {
        self.backing
    }

    pub(super) fn resolve_access(
        &self,
        body: &mut BlockBuilder<'_>,
        address: &Val<I32>,
        bytes: impl Into<Val<I32>>,
        intent: Intent,
    ) -> Result<Access, BuildError> {
        self.table.resolve_access(body, address, bytes, intent)
    }

    pub(super) fn read<T: TransferType>(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        access.check_field::<T>(offset);
        let address = access.linear.add(offset);
        body.if_value::<T>(
            &access.unavailable,
            |mut slow| {
                let value = T::read_parts(|offset, bytes| {
                    self.reader.call(&mut slow, (address.add(offset), bytes))
                })?;
                slow.yield_(value)
            },
            |mut direct_body| {
                let value = self.load::<T>(&mut direct_body, &access.physical, offset)?;
                direct_body.yield_(value)
            },
        )
    }

    pub(super) fn write<T: TransferType>(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
        offset: u32,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        assert!(matches!(access.intent, Intent::Write));
        access.check_field::<T>(offset);
        let address = access.linear.add(offset);
        body.if_else(
            &access.unavailable,
            |mut slow| {
                T::write_parts(value, |offset, bytes, part| {
                    self.writer
                        .call(&mut slow, (address.add(offset), bytes, part))
                })
            },
            |mut direct_body| {
                direct_body.store_at::<T>(self.backing, &access.physical, offset, value)
            },
        )
    }

    /// The caller has proved this read lies in a direct backing window.
    pub(super) fn load<T: MemoryType>(
        &self,
        body: &mut BlockBuilder<'_>,
        backing: &Val<I32>,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        body.load_at::<T>(self.backing, backing, offset)
    }

    /// Stores only under a successful direct writable-window proof.
    pub(super) fn store<T: MemoryType>(
        &self,
        body: &mut BlockBuilder<'_>,
        backing: &Val<I32>,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        body.store_at::<T>(self.backing, backing, 0, value)
    }

    pub(super) fn atomic_update<T: MemoryInt + TransferType>(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
        update: &OperandUpdate<T>,
    ) -> Result<Val<T>, BuildError> {
        assert_eq!(access.constant_bytes, Some(T::BYTES));
        // Physical execution runs one CPU with private backing. Callbacks cannot
        // reenter execution or advance another bus master during this entry.
        let previous = self.read(body, access, 0)?;
        self.write(body, access, 0, &update.apply(&previous))?;
        Ok(previous)
    }
}

#[cfg(test)]
mod tests;
