//! One general reader and writer per module handle all non-direct transfers.

use wasm86_compiler::{
    BlockBuilder, BuildError, Func, Program, Signature, Type, Val, I32, I64, I8,
};

use super::{
    super::physical_map::{MMIO, RAM},
    table::Entry,
    PhysicalMemory,
};

/// Covers one through eight bytes without a shift by the carrier's full width.
fn byte_mask(bytes: &Val<I32>) -> Val<I64> {
    let shift = Val::<I32>::from(8).sub(bytes).shl(3);
    Val::<I64>::from(u64::MAX).unsigned().shr(shift)
}

impl PhysicalMemory {
    pub(super) fn reader(&self, program: &mut Program) -> Result<Func, BuildError> {
        if let Some(reader) = self.reader.get() {
            return Ok(reader);
        }
        let reader = program.function(
            Signature {
                parameters: vec![Type::I32, Type::I32],
                results: vec![Type::I64],
            },
            |body| self.define_reader(body),
        )?;
        self.reader.set(Some(reader));
        Ok(reader)
    }

    pub(super) fn writer(&self, program: &mut Program) -> Result<Func, BuildError> {
        if let Some(writer) = self.writer.get() {
            return Ok(writer);
        }
        let writer = program.function(
            Signature {
                parameters: vec![Type::I32, Type::I32, Type::I64],
                results: vec![],
            },
            |body| self.define_writer(body),
        )?;
        self.writer.set(Some(writer));
        Ok(writer)
    }

    fn define_reader(&self, mut body: BlockBuilder<'_>) -> Result<(), BuildError> {
        let address = body.parameter::<I32>(0)?;
        let bytes = body.parameter::<I32>(1)?;
        let value = body.loop_::<(I32, I64), I64>((0, 0), |mut part, labels, (done, value)| {
            let current = address.add(&done);
            let entry = self.table.lookup(&mut part, &current)?;
            let count =
                self.table
                    .transfer_bytes(&mut part, &entry, &current, &bytes.sub(&done))?;
            let read = self.read_part(&mut part, &entry, &current, &count)?;
            let value = value.or(read.and(byte_mask(&count)).shl(done.shl(3)));
            let next = done.add(count);
            part.branch_if(next.eq(&bytes), &labels.exit, &value)?;
            part.branch(&labels.again, (next, value))
        })?;
        body.return_(value)
    }

    fn read_part(
        &self,
        body: &mut BlockBuilder<'_>,
        entry: &Entry,
        address: &Val<I32>,
        bytes: &Val<I32>,
    ) -> Result<Val<I64>, BuildError> {
        body.if_value::<I64>(
            entry.kind.eq(MMIO),
            |mut mmio| {
                let value = mmio.call::<I64>(self.mmio_reader, &[address.into(), bytes.into()])?;
                mmio.yield_(value)
            },
            |mut ordinary| {
                let value = ordinary.if_value::<I64>(
                    entry.readable(),
                    |mut ram| {
                        let value =
                            ram.load_at::<I8>(self.backing, entry.backing_address(address), 0)?;
                        ram.yield_(value.unsigned().extend::<I64>())
                    },
                    |hole| hole.yield_(u64::MAX),
                )?;
                ordinary.yield_(value)
            },
        )
    }

    fn define_writer(&self, mut body: BlockBuilder<'_>) -> Result<(), BuildError> {
        let address = body.parameter::<I32>(0)?;
        let bytes = body.parameter::<I32>(1)?;
        let value = body.parameter::<I64>(2)?;
        body.loop_::<I32, ()>(0, |mut part, labels, done| {
            let current = address.add(&done);
            let entry = self.table.lookup(&mut part, &current)?;
            let count =
                self.table
                    .transfer_bytes(&mut part, &entry, &current, &bytes.sub(&done))?;
            let shifted = value.unsigned().shr(done.shl(3));
            self.write_part(&mut part, &entry, &current, &count, &shifted)?;
            let next = done.add(count);
            part.branch_if(next.eq(&bytes), &labels.exit, ())?;
            part.branch(&labels.again, next)
        })?;
        body.return_(())
    }

    fn write_part(
        &self,
        body: &mut BlockBuilder<'_>,
        entry: &Entry,
        address: &Val<I32>,
        bytes: &Val<I32>,
        value: &Val<I64>,
    ) -> Result<(), BuildError> {
        body.if_else(
            entry.kind.eq(MMIO),
            |mut mmio| {
                mmio.call::<()>(
                    self.mmio_writer,
                    &[
                        address.into(),
                        bytes.into(),
                        value.and(byte_mask(bytes)).into(),
                    ],
                )
            },
            |mut ordinary| {
                ordinary.if_(entry.kind.eq(RAM), |mut ram| {
                    ram.store_at::<I8>(
                        self.backing,
                        entry.backing_address(address),
                        0,
                        value.truncate::<I8>(),
                    )
                })
            },
        )
    }
}
