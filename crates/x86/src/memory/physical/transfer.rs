//! One general reader and writer per module handle all non-direct transfers.

use relocade_compiler::{
    BlockBuilder, BuildError, Mem, Program, Table, TypedFunc, Val, I32, I64, I8,
};

use super::{
    super::{
        physical_map::{CODE_PAGE, MMIO, RAM},
        Intent,
    },
    table::{Entry, PhysicalTable},
};

/// Builds the shared transfer helpers and owns their backing and host operations.
#[derive(Clone, Copy)]
pub(super) struct TransferBuilder {
    backing: Mem,
    table: PhysicalTable,
    mmio_reader: Table,
    mmio_writer: Table,
    code_writer: TypedFunc<crate::abi::CodeWriteSignature>,
}

impl TransferBuilder {
    pub(super) fn new(program: &mut Program, backing: Mem, table: PhysicalTable) -> Self {
        Self {
            backing,
            table,
            mmio_reader: program.import_table(crate::abi::MMIO_READS),
            mmio_writer: program.import_table(crate::abi::MMIO_WRITES),
            code_writer: crate::abi::WRITE_CODE.import(program),
        }
    }

    pub(super) fn declare_reader(self, program: &mut Program) -> TypedFunc<fn(u32, u32) -> u64> {
        program.function::<fn(u32, u32) -> u64>(move |mut body, (address, bytes)| {
            let value =
                body.loop_::<(I32, I64), I64>((0, 0), |mut part, labels, (done, value)| {
                    let current = address.add(&done);
                    let entry = self.table.lookup(&mut part, &current)?;
                    let count = self.table.transfer_bytes(
                        &mut part,
                        &entry,
                        &bytes.sub(&done),
                        Intent::Read,
                    )?;
                    let read = self.read_part(&mut part, &entry, &count)?;
                    let value = value.or(read.and(byte_mask(&count)).shl(done.shl(3)));
                    let next = done.add(count);
                    part.branch_if(next.eq(&bytes), &labels.exit, &value)?;
                    part.branch(&labels.again, (next, value))
                })?;
            body.return_(value)
        })
    }

    fn read_part(
        self,
        body: &mut BlockBuilder<'_>,
        entry: &Entry,
        bytes: &Val<I32>,
    ) -> Result<Val<I64>, BuildError> {
        body.if_value::<I64>(
            entry.kind.eq(MMIO),
            |mut mmio| {
                let value = mmio.call_indirect::<I64>(
                    self.mmio_reader,
                    &entry.payload,
                    &[(&entry.address).into(), bytes.into()],
                )?;
                mmio.yield_(value)
            },
            |mut ordinary| {
                let value = ordinary.if_value::<I64>(
                    entry.readable(),
                    |mut ram| {
                        let value = ram.load_at::<I8>(self.backing, entry.backing_address(), 0)?;
                        ram.yield_(value.unsigned().extend::<I64>())
                    },
                    |hole| hole.yield_(u64::MAX),
                )?;
                ordinary.yield_(value)
            },
        )
    }

    pub(super) fn declare_writer(self, program: &mut Program) -> TypedFunc<fn(u32, u32, u64)> {
        program.function::<fn(u32, u32, u64)>(move |mut body, (address, bytes, value)| {
            body.loop_::<I32, ()>(0, |mut part, labels, done| {
                let current = address.add(&done);
                let entry = self.table.lookup(&mut part, &current)?;
                let count = self.table.transfer_bytes(
                    &mut part,
                    &entry,
                    &bytes.sub(&done),
                    Intent::Write,
                )?;
                let shifted = value.unsigned().shr(done.shl(3));
                self.write_part(&mut part, &entry, &count, &shifted)?;
                let next = done.add(count);
                part.branch_if(next.eq(&bytes), &labels.exit, ())?;
                part.branch(&labels.again, next)
            })?;
            body.return_(())
        })
    }

    fn write_part(
        self,
        body: &mut BlockBuilder<'_>,
        entry: &Entry,
        bytes: &Val<I32>,
        value: &Val<I64>,
    ) -> Result<(), BuildError> {
        const TRACKED_RAM: u32 = RAM | CODE_PAGE;
        body.switch(&entry.kind, &[RAM, TRACKED_RAM, MMIO], |mut part, kind| {
            match kind {
                Some(RAM) => part.store_at::<I8>(
                    self.backing,
                    entry.backing_address(),
                    0,
                    value.truncate::<I8>(),
                ),
                Some(TRACKED_RAM) => self.code_writer.call(
                    &mut part,
                    (entry.backing_address(), bytes, value.and(byte_mask(bytes))),
                ),
                Some(MMIO) => part.call_indirect::<()>(
                    self.mmio_writer,
                    &entry.payload,
                    &[
                        (&entry.address).into(),
                        bytes.into(),
                        value.and(byte_mask(bytes)).into(),
                    ],
                ),
                // ROM and unmapped pages ignore writes.
                _ => Ok(()),
            }
        })
    }
}

/// Covers one through eight bytes without a shift by the carrier's full width.
fn byte_mask(bytes: &Val<I32>) -> Val<I64> {
    let shift = Val::<I32>::from(8).sub(bytes).shl(3);
    Val::<I64>::from(u64::MAX).unsigned().shr(shift)
}
