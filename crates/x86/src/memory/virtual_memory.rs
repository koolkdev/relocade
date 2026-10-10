//! Guest virtual memory backed by a host-managed page table.

use std::cell::Cell;

use wasm86_compiler::{BuildError, Func, Mem, MemoryImport, Program, Signature, Type};

use super::page_table::PageTable;

/// Owns generated access helpers for one module. Frontends discard this owner
/// and its program together when construction fails.
pub(crate) struct VirtualMemory {
    pub(super) guest: Mem,
    pub(super) table: PageTable,
    pub(super) code: Option<super::code::CodeWrites>,
    pub(super) range_resolver: Func,
    pub(super) scattered_readers: [Cell<Option<Func>>; 4],
    pub(super) scattered_writers: [Cell<Option<Func>>; 4],
}

impl VirtualMemory {
    pub(super) fn declare(program: &mut Program, code_tracking: bool) -> Result<Self, BuildError> {
        let guest = program.import_memory(MemoryImport {
            module: "wasm86".into(),
            name: "guest".into(),
            minimum: 1,
            maximum: None,
            shared: false,
        });
        let table = PageTable::declare(program, code_tracking);
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
            code: code_tracking.then(|| super::code::CodeWrites::declare(program)),
            range_resolver,
            scattered_readers: std::array::from_fn(|_| Cell::new(None)),
            scattered_writers: std::array::from_fn(|_| Cell::new(None)),
        })
    }
}
