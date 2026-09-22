mod access;
mod page_table;
mod scattered;

pub(crate) use access::{Access, DirectRange};
pub(crate) use page_table::{PageCache, PageCacheInputs};

use std::cell::Cell;

use wasm86_compiler::{
    BuildError, Func, FunctionBuilder, Mem, MemoryImport, MemoryInt, Program, Signature, Type, Val,
    I32,
};

use page_table::{PageTable, PRESENT, WRITABLE};

/// Owns generated access helpers for one module. Frontends discard this owner
/// and its program together when construction fails.
pub(super) struct Memory {
    guest: Mem,
    table: PageTable,
    range_resolver: Func,
    scattered_readers: [Cell<Option<Func>>; 4],
    scattered_writers: [Cell<Option<Func>>; 4],
}

#[derive(Clone, Copy)]
pub(super) enum Intent {
    Fetch,
    Read,
    Write,
}

impl Intent {
    fn required_permissions(self) -> u32 {
        match self {
            Self::Fetch | Self::Read => PRESENT,
            Self::Write => PRESENT | WRITABLE,
        }
    }

    fn base_error_code(self) -> u32 {
        match self {
            Self::Fetch => 16,
            Self::Read => 0,
            Self::Write => 2,
        }
    }
}

impl Memory {
    pub(super) fn declare(program: &mut Program) -> Result<Self, BuildError> {
        let guest = program.import_memory(MemoryImport {
            module: "wasm86".into(),
            name: "guest".into(),
            minimum: 1,
            maximum: None,
            shared: false,
        });
        let table = PageTable::declare(program);
        let range_resolver = program.function(
            Signature {
                parameters: vec![Type::I32, Type::I32, Type::I32, Type::I32],
                results: vec![Type::I32],
            },
            |body| table.define_range_resolver(body),
        )?;
        Ok(Self {
            guest,
            table,
            range_resolver,
            scattered_readers: std::array::from_fn(|_| Cell::new(None)),
            scattered_writers: std::array::from_fn(|_| Cell::new(None)),
        })
    }

    pub(super) fn read<T: MemoryInt>(
        &self,
        body: &mut FunctionBuilder<'_>,
        access: &Access,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        access.check_field::<T>(offset);
        if access.bytes == 1 {
            return self.load(body, &access.physical, 0);
        }
        body.if_value::<T>(
            &access.scattered,
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

    pub(super) fn write<T: MemoryInt>(
        &self,
        body: &mut FunctionBuilder<'_>,
        access: &Access,
        offset: u32,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        access.check_field::<T>(offset);
        assert!(
            matches!(access.intent, Intent::Write),
            "store requires a write access"
        );
        if access.bytes == 1 {
            return body.store_at::<T>(self.guest, &access.physical, 0, value);
        }
        body.if_else(
            &access.scattered,
            |mut arm| {
                let writer = self.scattered_writer::<T>(arm.program())?;
                arm.call::<()>(writer, &[access.linear.add(offset).into(), value.into()])
            },
            |mut arm| arm.store_at::<T>(self.guest, &access.physical, offset, value),
        )
    }

    /// The caller must prove this entire read is present and physically contiguous.
    pub(super) fn load<T: MemoryInt>(
        &self,
        body: &mut FunctionBuilder<'_>,
        physical: &Val<I32>,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        body.load_at::<T>(self.guest, physical, offset)
    }
}

#[cfg(test)]
mod tests;
