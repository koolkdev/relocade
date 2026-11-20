//! Instruction bytes captured by the compiler and registrations for installed PC code.

mod pages;
pub(crate) use pages::CodeRegistry;
pub use pages::InstalledCode;
mod reader;
pub(crate) use reader::CodeReader;

use super::Memory;
use crate::{Error, Result};
use relocade_wasm_runtime::WasmSharedMemory;
use std::{
    ops::Range,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

const PAGE_BYTES: u32 = relocade_x86::PhysicalMemoryMap::PAGE_BYTES;

fn page_base(address: u32) -> u32 {
    address & !(PAGE_BYTES - 1)
}

/// The exact instruction bytes used by a compilation, in the memory where they were read.
#[derive(Clone)]
pub struct CodeSource {
    memory: Weak<Mutex<CodeRegistry>>,
    spans: Arc<[Span]>,
}

struct Span {
    address: u32,
    /// Physical page after A20, where this backing page's write route lives.
    physical_page: u32,
    backing: u32,
    bytes: Box<[u8]>,
}

impl Span {
    fn backing_range(&self) -> Range<u64> {
        u64::from(self.backing)..u64::from(self.backing) + self.bytes.len() as u64
    }

    fn changed_by(&self, backing: u32, bytes: &[u8]) -> bool {
        let source = self.backing_range();
        let write = u64::from(backing);
        let start = source.start.max(write);
        let end = source.end.min(write + bytes.len() as u64);
        start < end
            && self.bytes[(start - source.start) as usize..(end - source.start) as usize]
                != bytes[(start - write) as usize..(end - write) as usize]
    }
}

impl CodeSource {
    /// Exact instruction ranges, in block order, before A20 and physical mapping.
    pub fn ranges(&self) -> impl Iterator<Item = Range<u64>> + '_ {
        self.spans
            .iter()
            .map(|span| u64::from(span.address)..u64::from(span.address) + span.bytes.len() as u64)
    }

    /// The same dependencies in shared backing storage, including guest aliases.
    pub fn backing_ranges(&self) -> impl Iterator<Item = Range<u64>> + '_ {
        self.spans.iter().map(Span::backing_range)
    }

    pub(crate) fn belongs_to(&self, memory: &Memory) -> bool {
        Weak::ptr_eq(&self.memory, &Arc::downgrade(&memory.code))
    }

    /// Hold off managed writes until the caller publishes and registers the checked instructions.
    pub(crate) fn lock_current_bytes<'a>(
        &self,
        memory: &'a Memory,
    ) -> Result<MutexGuard<'a, CodeRegistry>> {
        let registry = memory.code.lock().unwrap();
        let mut scratch = [0; PAGE_BYTES as usize];
        for span in self.spans.iter() {
            let live = &mut scratch[..span.bytes.len()];
            memory.backing.read(span.backing, live);
            if live != span.bytes.as_ref() {
                return Err(Error::StaleCompilation);
            }
        }
        Ok(registry)
    }
}

impl Memory {
    pub(crate) fn code_reader(&self) -> CodeReader<'_> {
        CodeReader::new(self)
    }

    /// Active installations whose instructions cover this backing byte.
    pub fn installed_code_at(&self, backing: u32) -> Vec<Arc<InstalledCode>> {
        self.code
            .lock()
            .unwrap()
            .covering(backing)
            .cloned()
            .collect()
    }

    /// Instruction snapshots covering this backing byte, one per active installation.
    pub fn code_sources_at(&self, backing: u32) -> Vec<CodeSource> {
        self.code
            .lock()
            .unwrap()
            .covering(backing)
            .map(|code| code.source().clone())
            .collect()
    }

    pub(super) fn write_storage(&self, backing: u32, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut registry = self.code.lock().unwrap();
        self.backing.write(backing, bytes);
        registry.record_write(backing, bytes);
    }

    /// Consume the notification before examining installations so a later write remains pending.
    pub(crate) fn take_retirement_request(&self) -> bool {
        std::mem::take(&mut self.code.lock().unwrap().needs_retirement)
    }
}

#[cfg(test)]
mod tests;
