//! Instruction bytes captured by the compiler and registrations for installed PC code.

mod blacklist;
mod pages;
use pages::CodePages;
pub(super) use pages::CodeWrite;
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ModuleId(pub(crate) usize);

#[derive(Default)]
pub struct CodeRegistry {
    pages: Mutex<CodePages>,
}

/// The exact instruction bytes used by a compilation, in the memory where they were read.
#[derive(Clone)]
pub struct CodeSource {
    memory: Weak<CodeRegistry>,
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

    fn record_changes(&self, backing: u32, bytes: &[u8], mut record: impl FnMut(u32)) -> bool {
        let source = self.backing_range();
        let write = u64::from(backing);
        let start = source.start.max(write);
        let end = source.end.min(write + bytes.len() as u64);
        if start >= end {
            return false;
        }
        let compiled = &self.bytes[(start - source.start) as usize..(end - source.start) as usize];
        let written = &bytes[(start - write) as usize..(end - write) as usize];
        if compiled == written {
            return false;
        }
        for (index, (old, new)) in compiled.iter().zip(written).enumerate() {
            if old != new {
                record(start as u32 + index as u32);
            }
        }
        true
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
    ) -> Result<MutexGuard<'a, CodePages>> {
        let pages = memory.code.pages.lock().unwrap();
        let mut scratch = [0; PAGE_BYTES as usize];
        for span in self.spans.iter() {
            if pages.excluded().intersects(span.backing, span.bytes.len()) {
                return Err(Error::StaleCompilation);
            }
            let live = &mut scratch[..span.bytes.len()];
            memory.backing.read(span.backing, live);
            if live != span.bytes.as_ref() {
                return Err(Error::StaleCompilation);
            }
        }
        Ok(pages)
    }
}

impl Memory {
    pub(crate) fn code_reader(&self) -> CodeReader<'_> {
        CodeReader::new(self)
    }

    /// Active installations whose instructions cover this backing byte.
    pub fn installed_code_at(&self, backing: u32) -> Vec<Arc<InstalledCode>> {
        self.code
            .pages
            .lock()
            .unwrap()
            .covering(backing)
            .cloned()
            .collect()
    }

    /// Instruction snapshots covering this backing byte, one per active installation.
    pub fn code_sources_at(&self, backing: u32) -> Vec<CodeSource> {
        self.code
            .pages
            .lock()
            .unwrap()
            .covering(backing)
            .map(|code| code.source().clone())
            .collect()
    }

    pub(super) fn write_storage(&self, backing: u32, bytes: &[u8], write: &mut CodeWrite) {
        if bytes.is_empty() {
            return;
        }
        let mut pages = self.code.pages.lock().unwrap();
        self.backing.write(backing, bytes);
        let waiters = pages.mark_modified(backing, bytes, write);
        drop(pages);
        // A wake can run caller code. Release the shared registry before notifying any CPU.
        for waiter in waiters {
            waiter.wake();
        }
    }
}

#[cfg(test)]
mod tests;
