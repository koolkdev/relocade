//! Guest virtual memory backed by a host-managed page table.

use std::cell::OnceCell;

use relocade_compiler::{Func, Mem, Program, TypedFunc};

use super::page_table::PageTable;

/// Module handles for guest memory and its shared access helpers.
pub(crate) struct VirtualMemory {
    pub(super) guest: Mem,
    pub(super) table: PageTable,
    pub(super) range_resolver: TypedFunc<fn(u32, u32, u32, u32) -> u32>,
    pub(super) scattered_readers: [OnceCell<Func>; 5],
    pub(super) scattered_writers: [OnceCell<Func>; 5],
}

impl VirtualMemory {
    pub(super) fn declare(program: &mut Program) -> Self {
        let guest = program.import_memory(crate::abi::GUEST);
        let table = PageTable::declare(program);
        let range_resolver = table.range_resolver(program);
        Self {
            guest,
            table,
            range_resolver,
            scattered_readers: std::array::from_fn(|_| OnceCell::new()),
            scattered_writers: std::array::from_fn(|_| OnceCell::new()),
        }
    }
}
