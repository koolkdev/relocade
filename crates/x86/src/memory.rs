mod access;
mod accesses;
mod page_table;
mod transfer;
mod update;

pub(crate) use access::Access;
pub(crate) use accesses::Accesses;
pub(crate) use page_table::{PageCache, PageCacheInputs};

use std::cell::Cell;

use wasm86_compiler::{BuildError, Func, Mem, MemoryImport, Program, Signature, Type};

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
}

#[cfg(test)]
mod tests;
