//! Guest virtual memory backed by a host-managed page table.

use relocade_compiler::{Func, Mem, Program, TypedFunc, I16, I32, I64, I8, V128};

use super::{
    page_table::{PageTable, ResolveRange},
    transfer::{declare_reader, declare_writer},
};

/// Module handles for guest memory and its shared access helpers.
pub(crate) struct VirtualMemory {
    pub(super) guest: Mem,
    pub(super) table: PageTable,
    pub(super) range_resolver: TypedFunc<ResolveRange>,
    pub(super) scattered_readers: [Func; 5],
    pub(super) scattered_writers: [Func; 5],
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
            scattered_readers: [
                declare_reader::<I8>(program, guest, table),
                declare_reader::<I16>(program, guest, table),
                declare_reader::<I32>(program, guest, table),
                declare_reader::<I64>(program, guest, table),
                declare_reader::<V128>(program, guest, table),
            ],
            scattered_writers: [
                declare_writer::<I8>(program, guest, table),
                declare_writer::<I16>(program, guest, table),
                declare_writer::<I32>(program, guest, table),
                declare_writer::<I64>(program, guest, table),
                declare_writer::<V128>(program, guest, table),
            ],
        }
    }
}
