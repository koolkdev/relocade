//! Instruction bytes captured by the compiler and registrations for installed PC code.

mod history;
use history::{CodeHistory, ExcludedCode};
mod pages;
use pages::CodePages;
pub use pages::InstalledCode;
mod reader;
pub(crate) use reader::CodeReader;

use super::Memory;
use crate::{Error, Result};
use relocade_wasm_runtime::WasmSharedMemory;
use std::{
    ops::Range,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard, Weak,
    },
};

const PAGE_BYTES: u32 = relocade_x86::PhysicalMemoryMap::PAGE_BYTES;

fn page_base(address: u32) -> u32 {
    address & !(PAGE_BYTES - 1)
}

#[derive(Default)]
pub struct CodeRegistry {
    pages: Mutex<CodePages>,
    needs_retirement: AtomicBool,
}

/// The exact instruction bytes used by a compilation, in the memory where they were read.
#[derive(Clone)]
pub struct CodeSource {
    memory: Weak<CodeRegistry>,
    spans: Arc<[Span]>,
    blocks: Arc<[SourceBlock]>,
}

struct SourceBlock {
    spans: Range<usize>,
    instruction_lengths: Arc<[u8]>,
}

impl SourceBlock {
    fn instructions(&self, address: u32) -> impl Iterator<Item = Range<u64>> + '_ {
        self.instruction_lengths
            .iter()
            .scan(u64::from(address), |ip, &len| {
                let start = *ip;
                *ip += u64::from(len);
                Some(start..*ip)
            })
    }
}

struct Span {
    block: usize,
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

    /// Map a linear interval back to the part supplied by this page.
    fn backing_part(&self, linear: &Range<u64>) -> Option<Range<u64>> {
        let base = u64::from(self.address);
        let first = linear.start.max(base);
        let last = linear.end.min(base + self.bytes.len() as u64);
        let backing = u64::from(self.backing);
        (first < last).then(|| backing + (first - base)..backing + (last - base))
    }

    /// The changed overlap, in linear guest address order.
    fn changed_slice<'a>(
        &'a self,
        backing: u32,
        bytes: &'a [u8],
    ) -> Option<(u64, &'a [u8], &'a [u8])> {
        let source = self.backing_range();
        let write = u64::from(backing);
        let start = source.start.max(write);
        let end = source.end.min(write + bytes.len() as u64);
        if start >= end {
            return None;
        }
        let offset = (start - source.start) as usize;
        let before = &self.bytes[offset..(end - source.start) as usize];
        let after = &bytes[(start - write) as usize..(end - write) as usize];
        (before != after).then_some((u64::from(self.address) + offset as u64, before, after))
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

    /// If the write changes any byte in this span, return the full backing ranges
    /// of the affected instructions. A cross-page instruction can have two ranges.
    fn changed_instruction_ranges<'a>(
        &'a self,
        span_index: usize,
        backing: u32,
        bytes: &'a [u8],
    ) -> Option<impl Iterator<Item = Range<u64>> + 'a> {
        let span = &self.spans[span_index];
        let (write, before, after) = span.changed_slice(backing, bytes)?;
        let end = write + before.len() as u64;
        let block = &self.blocks[span.block];
        let address = self.spans[block.spans.start].address;
        let changed = block
            .instructions(address)
            .take_while(move |instruction| instruction.start < end)
            .filter(move |instruction| {
                if instruction.end <= write {
                    return false;
                }
                let first = (instruction.start.max(write) - write) as usize;
                let last = (instruction.end.min(end) - write) as usize;
                before[first..last] != after[first..last]
            });

        // At most one neighboring page supplies the rest of an x86 instruction.
        let first = span_index.saturating_sub(1).max(block.spans.start);
        let past = (span_index + 2).min(block.spans.end);
        let neighbors = &self.spans[first..past];
        Some(changed.flat_map(move |instruction| {
            neighbors
                .iter()
                .filter_map(move |span| span.backing_part(&instruction))
        }))
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

    pub(super) fn write_storage(&self, backing: u32, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut pages = self.code.pages.lock().unwrap();
        self.backing.write(backing, bytes);
        if pages.mark_modified(backing, bytes) {
            self.code.needs_retirement.store(true, Ordering::Release);
        }
    }

    /// A clean guest entry only reads the flag. Consume a pending request before
    /// scanning installations so a write during retirement can request another scan.
    pub(crate) fn take_retirement_request(&self) -> bool {
        let pending = &self.code.needs_retirement;
        pending.load(Ordering::Relaxed) && pending.swap(false, Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests;
