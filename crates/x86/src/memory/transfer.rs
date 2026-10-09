//! Typed transfers through resolved spans and shared helpers for scattered backing.
//! Guest transfers cannot alter the separate page table, so checked scattered
//! transfers need only a frame lookup for each byte.

use wasm86_compiler::{
    BlockBuilder, BuildError, Func, MemoryInt, Program, Signature, Type, Val, I32, I8,
};

use super::{page_table::physical_address, Access, Intent, VirtualMemory};

impl VirtualMemory {
    pub(crate) fn read<T: MemoryInt>(
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
                let reader = self.scattered_reader::<T>(arm.program())?;
                let value = arm.call::<T>(reader, &[access.linear.add(offset).into()])?;
                arm.yield_(value)
            },
            |mut arm| {
                let value = self.load::<T>(&mut arm, &access.physical, offset)?;
                arm.yield_(value)
            },
        )
    }

    pub(crate) fn write<T: MemoryInt>(
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
        if let Some(code) = self.code {
            code.before_write(body, &access.watched, &access.linear.add(offset), T::BYTES)?;
        }
        if access.constant_bytes == Some(1) {
            return body.store_at::<T>(self.guest, &access.physical, 0, value);
        }
        body.if_else(
            access.scattered(),
            |mut arm| {
                let writer = self.scattered_writer::<T>(arm.program())?;
                arm.call::<()>(writer, &[access.linear.add(offset).into(), value.into()])
            },
            |mut arm| arm.store_at::<T>(self.guest, &access.physical, offset, value),
        )
    }

    /// The caller must prove this entire read is present and physically contiguous.
    pub(crate) fn load<T: MemoryInt>(
        &self,
        body: &mut BlockBuilder<'_>,
        physical: &Val<I32>,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        body.load_at::<T>(self.guest, physical, offset)
    }

    /// The caller must prove this entire write is writable and contiguous.
    pub(crate) fn store<T: MemoryInt>(
        &self,
        body: &mut BlockBuilder<'_>,
        physical: &Val<I32>,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        body.store_at::<T>(self.guest, physical, 0, value)
    }

    fn scattered_reader<T: MemoryInt>(&self, program: &mut Program) -> Result<Func, BuildError> {
        let slot = &self.scattered_readers[width_index::<T>()];
        if let Some(function) = slot.get() {
            return Ok(function);
        }
        let function = program.function(
            Signature {
                parameters: vec![Type::I32],
                results: vec![T::TYPE],
            },
            |body| self.define_scattered_reader::<T>(body),
        )?;
        slot.set(Some(function));
        Ok(function)
    }

    fn scattered_writer<T: MemoryInt>(&self, program: &mut Program) -> Result<Func, BuildError> {
        let slot = &self.scattered_writers[width_index::<T>()];
        if let Some(function) = slot.get() {
            return Ok(function);
        }
        let function = program.function(
            Signature {
                parameters: vec![Type::I32, T::TYPE],
                results: vec![],
            },
            |body| self.define_scattered_writer::<T>(body),
        )?;
        slot.set(Some(function));
        Ok(function)
    }

    fn define_scattered_reader<T: MemoryInt>(
        &self,
        mut body: BlockBuilder<'_>,
    ) -> Result<(), BuildError> {
        let linear = body.parameter::<I32>(0)?;
        let mut value = body.value::<T>(0)?;
        for offset in 0..T::BYTES {
            let address = linear.add(offset);
            let entry = self.table.entry(&mut body, &address)?;
            let byte = self.load::<I8>(&mut body, &physical_address(&entry, &address), 0)?;
            value = value.or(byte.unsigned().extend::<T>().shl(offset * 8));
        }
        body.return_(value)
    }

    fn define_scattered_writer<T: MemoryInt>(
        &self,
        mut body: BlockBuilder<'_>,
    ) -> Result<(), BuildError> {
        let linear = body.parameter::<I32>(0)?;
        let value = body.parameter::<T>(1)?;
        for offset in 0..T::BYTES {
            let address = linear.add(offset);
            let entry = self.table.entry(&mut body, &address)?;
            body.store_at::<I8>(
                self.guest,
                physical_address(&entry, &address),
                0,
                value.unsigned().shr(offset * 8).truncate::<I8>(),
            )?;
        }
        body.return_(())
    }
}

fn width_index<T: MemoryInt>() -> usize {
    match T::TYPE {
        Type::I8 => 0,
        Type::I16 => 1,
        Type::I32 => 2,
        Type::I64 => 3,
        _ => unreachable!("guest transfers use byte, word, dword or qword fields"),
    }
}
