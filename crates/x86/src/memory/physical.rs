//! Direct physical backing accesses and shared routing for MMIO and split spans.

mod table;
mod transfer;

use std::cell::Cell;

use wasm86_compiler::{
    BlockBuilder, BuildError, Func, FunctionImport, Mem, MemoryImport, MemoryInt, Program,
    Signature, Type, Val, I32, I64,
};

use super::{Access, DirectRange, Intent};
use crate::alu::OperandUpdate;
use table::PhysicalTable;

pub(crate) struct PhysicalMemory {
    backing: Mem,
    table: PhysicalTable,
    pub(super) code: Option<super::code::CodeWrites>,
    mmio_reader: Func,
    mmio_writer: Func,
    reader: Cell<Option<Func>>,
    writer: Cell<Option<Func>>,
}

impl PhysicalMemory {
    pub(super) fn declare(program: &mut Program, code_tracking: bool) -> Self {
        let backing = program.import_memory(MemoryImport {
            module: "wasm86".into(),
            name: "guest".into(),
            minimum: 1,
            maximum: None,
            shared: false,
        });
        let table = PhysicalTable::declare(program, code_tracking);
        let mmio_reader = program.import_function(FunctionImport {
            module: "wasm86".into(),
            name: "readMmio".into(),
            signature: Signature {
                parameters: vec![Type::I32, Type::I32],
                results: vec![Type::I64],
            },
        });
        let mmio_writer = program.import_function(FunctionImport {
            module: "wasm86".into(),
            name: "writeMmio".into(),
            signature: Signature {
                parameters: vec![Type::I32, Type::I32, Type::I64],
                results: vec![],
            },
        });
        Self {
            backing,
            table,
            code: code_tracking.then(|| super::code::CodeWrites::declare(program)),
            mmio_reader,
            mmio_writer,
            reader: Cell::new(None),
            writer: Cell::new(None),
        }
    }

    pub(super) fn backing(&self) -> Mem {
        self.backing
    }

    pub(super) fn check_direct_access(
        &self,
        body: &mut BlockBuilder<'_>,
        address: &Val<I32>,
        bytes: impl Into<Val<I32>>,
        intent: Intent,
    ) -> Result<DirectRange, BuildError> {
        self.table.direct_range(body, address, bytes, intent)
    }

    pub(super) fn read<T: MemoryInt>(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        access.check_field::<T>(offset);
        if body.constant_bits(&access.unavailable)? == Some(0) {
            return self.load(body, &access.physical, offset);
        }
        let address = access.linear.add(offset);
        let direct = self.check_direct_access(body, &address, T::BYTES, Intent::Read)?;
        body.if_value::<T>(
            direct.unavailable,
            |mut slow| {
                let reader = self.reader(slow.program())?;
                let value = slow.call::<I64>(reader, &[address.into(), T::BYTES.into()])?;
                slow.yield_(value.truncate::<T>())
            },
            |mut direct_body| {
                let value = self.load::<T>(&mut direct_body, &direct.physical, 0)?;
                direct_body.yield_(value)
            },
        )
    }

    pub(super) fn write<T: MemoryInt>(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
        offset: u32,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        assert!(matches!(access.intent, Intent::Write));
        access.check_field::<T>(offset);
        if body.constant_bits(&access.unavailable)? == Some(0) {
            return body.store_at::<T>(self.backing, &access.physical, offset, value);
        }
        let address = access.linear.add(offset);
        let direct = self.check_direct_access(body, &address, T::BYTES, Intent::Write)?;
        body.if_else(
            direct.unavailable,
            |mut slow| {
                let writer = self.writer(slow.program())?;
                slow.call::<()>(
                    writer,
                    &[
                        address.into(),
                        T::BYTES.into(),
                        value.unsigned().extend::<I64>().into(),
                    ],
                )
            },
            |mut direct_body| direct_body.store_at::<T>(self.backing, direct.physical, 0, value),
        )
    }

    /// The caller has proved this read lies in a direct backing window.
    pub(super) fn load<T: MemoryInt>(
        &self,
        body: &mut BlockBuilder<'_>,
        backing: &Val<I32>,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        body.load_at::<T>(self.backing, backing, offset)
    }

    /// Stores only under a successful direct writable-window proof.
    pub(super) fn store<T: MemoryInt>(
        &self,
        body: &mut BlockBuilder<'_>,
        backing: &Val<I32>,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        body.store_at::<T>(self.backing, backing, 0, value)
    }

    pub(super) fn atomic_update<T: MemoryInt>(
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
