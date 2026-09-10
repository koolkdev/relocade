//! Shared byte transfers after the complete linear span passes permission checks.
//! Guest transfers cannot alter the separate page table, so each byte needs only
//! a frame lookup.

use wasm86_compiler::{
    BuildError, Func, FunctionBuilder, MemoryInt, Program, Signature, Type, I32, I8,
};

use super::{physical_address, Memory};

impl Memory {
    pub(super) fn scattered_reader<T: MemoryInt>(
        &self,
        program: &mut Program,
    ) -> Result<Func, BuildError> {
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

    pub(super) fn scattered_writer<T: MemoryInt>(
        &self,
        program: &mut Program,
    ) -> Result<Func, BuildError> {
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
        mut body: FunctionBuilder<'_>,
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
        mut body: FunctionBuilder<'_>,
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
        Type::I16 => 0,
        Type::I32 => 1,
        Type::I64 => 2,
        _ => unreachable!("byte accesses do not need scattered transfers"),
    }
}
