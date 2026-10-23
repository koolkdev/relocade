//! One general reader and writer per module handle all non-direct transfers.

use relocade_compiler::{
    BlockBuilder, BuildError, Func, Mem, Program, TypedFunc, Val, I32, I64, I8,
};

use super::{
    super::physical_map::{MMIO, RAM},
    table::{Entry, PhysicalTable},
};

/// Covers one through eight bytes without a shift by the carrier's full width.
fn byte_mask(bytes: &Val<I32>) -> Val<I64> {
    let shift = Val::<I32>::from(8).sub(bytes).shl(3);
    Val::<I64>::from(u64::MAX).unsigned().shr(shift)
}

pub(super) fn declare_reader(
    program: &mut Program,
    backing: Mem,
    table: PhysicalTable,
    mmio_reader: Func,
) -> TypedFunc<fn(u32, u32) -> u64> {
    program.function::<fn(u32, u32) -> u64>(move |mut body, (address, bytes)| {
        let value = body.loop_::<(I32, I64), I64>((0, 0), |mut part, labels, (done, value)| {
            let current = address.add(&done);
            let entry = table.lookup(&mut part, &current)?;
            let count = table.transfer_bytes(&mut part, &entry, &bytes.sub(&done))?;
            let read = read_part(backing, mmio_reader, &mut part, &entry, &count)?;
            let value = value.or(read.and(byte_mask(&count)).shl(done.shl(3)));
            let next = done.add(count);
            part.branch_if(next.eq(&bytes), &labels.exit, &value)?;
            part.branch(&labels.again, (next, value))
        })?;
        body.return_(value)
    })
}

fn read_part(
    backing: Mem,
    callback: Func,
    body: &mut BlockBuilder<'_>,
    entry: &Entry,
    bytes: &Val<I32>,
) -> Result<Val<I64>, BuildError> {
    body.if_value::<I64>(
        entry.kind.eq(MMIO),
        |mut mmio| {
            let value = mmio.call::<I64>(callback, &[(&entry.address).into(), bytes.into()])?;
            mmio.yield_(value)
        },
        |mut ordinary| {
            let value = ordinary.if_value::<I64>(
                entry.readable(),
                |mut ram| {
                    let value = ram.load_at::<I8>(backing, entry.backing_address(), 0)?;
                    ram.yield_(value.unsigned().extend::<I64>())
                },
                |hole| hole.yield_(u64::MAX),
            )?;
            ordinary.yield_(value)
        },
    )
}

pub(super) fn declare_writer(
    program: &mut Program,
    backing: Mem,
    table: PhysicalTable,
    mmio_writer: Func,
) -> TypedFunc<fn(u32, u32, u64)> {
    program.function::<fn(u32, u32, u64)>(move |mut body, (address, bytes, value)| {
        body.loop_::<I32, ()>(0, |mut part, labels, done| {
            let current = address.add(&done);
            let entry = table.lookup(&mut part, &current)?;
            let count = table.transfer_bytes(&mut part, &entry, &bytes.sub(&done))?;
            let shifted = value.unsigned().shr(done.shl(3));
            write_part(backing, mmio_writer, &mut part, &entry, &count, &shifted)?;
            let next = done.add(count);
            part.branch_if(next.eq(&bytes), &labels.exit, ())?;
            part.branch(&labels.again, next)
        })?;
        body.return_(())
    })
}

fn write_part(
    backing: Mem,
    callback: Func,
    body: &mut BlockBuilder<'_>,
    entry: &Entry,
    bytes: &Val<I32>,
    value: &Val<I64>,
) -> Result<(), BuildError> {
    body.if_else(
        entry.kind.eq(MMIO),
        |mut mmio| {
            mmio.call::<()>(
                callback,
                &[
                    (&entry.address).into(),
                    bytes.into(),
                    value.and(byte_mask(bytes)).into(),
                ],
            )
        },
        |mut ordinary| {
            ordinary.if_(entry.kind.eq(RAM), |mut ram| {
                ram.store_at::<I8>(backing, entry.backing_address(), 0, value.truncate::<I8>())
            })
        },
    )
}
