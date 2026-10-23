//! Guest virtual memory backed by a host-managed page table.

use std::cell::Cell;

use relocade_compiler::{BuildError, Func, Mem, Program, Signature, Type};

use super::page_table::PageTable;

/// Owns generated access helpers for one module. Frontends discard this owner
/// and its program together when construction fails.
pub(crate) struct VirtualMemory {
    pub(super) guest: Mem,
    pub(super) table: PageTable,
    range_resolver: Cell<Option<Func>>,
    pub(super) scattered_readers: [Cell<Option<Func>>; 5],
    pub(super) scattered_writers: [Cell<Option<Func>>; 5],
}

impl VirtualMemory {
    pub(super) fn declare(program: &mut Program) -> Self {
        Self {
            guest: program.import_memory(crate::abi::GUEST),
            table: PageTable::declare(program),
            range_resolver: Cell::new(None),
            scattered_readers: std::array::from_fn(|_| Cell::new(None)),
            scattered_writers: std::array::from_fn(|_| Cell::new(None)),
        }
    }

    pub(super) fn range_resolver(&self, program: &mut Program) -> Result<Func, BuildError> {
        if let Some(function) = self.range_resolver.get() {
            return Ok(function);
        }
        let function = program.function(
            Signature {
                parameters: vec![Type::I32, Type::I32, Type::I32, Type::I32],
                results: vec![Type::I32],
            },
            |body| self.table.define_range_resolver(body),
        )?;
        self.range_resolver.set(Some(function));
        Ok(function)
    }
}
