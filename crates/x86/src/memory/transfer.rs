//! Typed transfers through resolved spans and shared helpers for scattered backing.
//! Guest transfers cannot alter the separate page table, so checked scattered
//! transfers need only a frame lookup for each byte.

use crate::memory::TransferType;
use relocade_compiler::{
    BlockBuilder, BuildError, Func, Mem, MemoryType, Program, Type, Val, I32, I64, I8,
};

use super::{
    page_table::{physical_address, PageTable},
    Access, Intent, VirtualMemory,
};

impl VirtualMemory {
    pub(crate) fn read<T: TransferType>(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        access.check_field::<T>(offset);
        if access.constant_bytes == Some(1) {
            return self.load(body, &access.physical, 0);
        }
        body.if_value::<T>(
            access.scattered(),
            |mut arm| {
                let reader = *self.scattered_readers[width_index::<T>()]
                    .get_or_init(|| declare_reader::<T>(arm.program(), self.guest, self.table));
                let value = arm.call::<T>(reader, &[access.linear.add(offset).into()])?;
                arm.yield_(value)
            },
            |mut arm| {
                let value = self.load::<T>(&mut arm, &access.physical, offset)?;
                arm.yield_(value)
            },
        )
    }

    pub(crate) fn write<T: TransferType>(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
        offset: u32,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        access.check_field::<T>(offset);
        assert!(
            matches!(access.intent, Intent::Write),
            "store requires a write access"
        );
        if access.constant_bytes == Some(1) {
            return body.store_at::<T>(self.guest, &access.physical, 0, value);
        }
        body.if_else(
            access.scattered(),
            |mut arm| {
                let writer = *self.scattered_writers[width_index::<T>()]
                    .get_or_init(|| declare_writer::<T>(arm.program(), self.guest, self.table));
                arm.call::<()>(writer, &[access.linear.add(offset).into(), value.into()])
            },
            |mut arm| arm.store_at::<T>(self.guest, &access.physical, offset, value),
        )
    }

    /// The caller must prove this entire read is present and physically contiguous.
    pub(crate) fn load<T: MemoryType>(
        &self,
        body: &mut BlockBuilder<'_>,
        physical: &Val<I32>,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        body.load_at::<T>(self.guest, physical, offset)
    }

    /// The caller must prove this entire write is writable and contiguous.
    pub(crate) fn store<T: MemoryType>(
        &self,
        body: &mut BlockBuilder<'_>,
        physical: &Val<I32>,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        body.store_at::<T>(self.guest, physical, 0, value)
    }
}

pub(super) fn declare_reader<T: TransferType>(
    program: &mut Program,
    guest: Mem,
    table: PageTable,
) -> Func {
    program
        .function::<fn(u32) -> T>(move |mut body, (linear,)| {
            let value = T::read_parts(|part, bytes| {
                let mut value = Val::<I64>::from(0);
                for offset in 0..bytes {
                    let address = linear.add(part + offset);
                    let entry = table.entry(&mut body, &address)?;
                    let byte = body.load_at::<I8>(guest, physical_address(&entry, &address), 0)?;
                    value = value.or(byte.unsigned().extend::<I64>().shl(offset * 8));
                }
                Ok(value)
            })?;
            body.return_(value)
        })
        .into()
}

pub(super) fn declare_writer<T: TransferType>(
    program: &mut Program,
    guest: Mem,
    table: PageTable,
) -> Func {
    program
        .function::<fn(u32, T)>(move |mut body, (linear, value)| {
            T::write_parts(&value, |part, bytes, value| {
                for offset in 0..bytes {
                    let address = linear.add(part + offset);
                    let entry = table.entry(&mut body, &address)?;
                    body.store_at::<I8>(
                        guest,
                        physical_address(&entry, &address),
                        0,
                        value.unsigned().shr(offset * 8).truncate::<I8>(),
                    )?;
                }
                Ok(())
            })?;
            body.return_(())
        })
        .into()
}

fn width_index<T: TransferType>() -> usize {
    match T::TYPE {
        Type::I8 => 0,
        Type::I16 => 1,
        Type::I32 => 2,
        Type::I64 => 3,
        Type::V128 => 4,
        _ => unreachable!("guest transfers use integer or vector fields"),
    }
}
