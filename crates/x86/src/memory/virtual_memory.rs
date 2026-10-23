//! Guest virtual memory backed by a host-managed page table.

use std::cell::Cell;

use relocade_compiler::{BuildError, Func, Mem, Program, TypedFunc};

use super::page_table::PageTable;

/// Owns generated access helpers for one module. Frontends discard this owner
/// and its program together when construction fails.
pub(crate) struct VirtualMemory {
    pub(super) guest: Mem,
    pub(super) table: PageTable,
    pub(super) range_resolver: TypedFunc<fn(u32, u32, u32, u32) -> u32>,
    pub(super) scattered_readers: [Cell<Option<Func>>; 5],
    pub(super) scattered_writers: [Cell<Option<Func>>; 5],
}

impl VirtualMemory {
    pub(super) fn declare(program: &mut Program) -> Result<Self, BuildError> {
        let guest = program.import_memory(crate::abi::GUEST);
        let table = PageTable::declare(program);
        let range_resolver = table.range_resolver(program)?;
        Ok(Self {
            guest,
            table,
            range_resolver,
            scattered_readers: std::array::from_fn(|_| Cell::new(None)),
            scattered_writers: std::array::from_fn(|_| Cell::new(None)),
        })
    }
}
