//! Shared instruction sources and the backing pages retained by their snapshots.

mod reader;
mod registration;
pub(crate) use reader::CodeReader;

use super::Memory;
use crate::{Error, Result};
use futures_util::lock::Mutex as AsyncMutex;
use relocade_wasm_runtime::{SharedMemory, WasmSharedMemory};
use relocade_x86::PhysicalMemoryMap;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
};

const PAGE_BYTES: u32 = relocade_x86::PhysicalMemoryMap::PAGE_BYTES;

/// Immutable instruction bytes used by a compilation. Clones retain the same
/// source registration, including while results are pending or installed on CPUs.
#[derive(Clone)]
pub struct CodeSource {
    registry: Arc<CodeRegistry>,
    source: Arc<Source>,
}

struct Source {
    bytes: InstructionBytes,
    pages: Vec<PageVersion>,
}

pub(crate) struct InstructionBytes {
    spans: Vec<Span>,
}

impl InstructionBytes {
    fn pages(&self) -> BTreeSet<u32> {
        self.spans
            .iter()
            .map(|span| span.backing / PAGE_BYTES * PAGE_BYTES)
            .collect()
    }

    // The machine's address policy is immutable; each span resolves to this
    // same backing range for the lifetime of the memory.
    fn matches(&self, backing: &SharedMemory) -> bool {
        let mut scratch = [0; PAGE_BYTES as usize];
        self.spans.iter().all(|span| {
            let live = &mut scratch[..span.bytes.len()];
            backing.read(span.backing, live);
            *live == *span.bytes
        })
    }
}

struct Span {
    address: u32,
    backing: u32,
    bytes: Box<[u8]>,
}

struct PageVersion {
    backing: u32,
    registered: u64,
    current: Arc<TrackedPage>,
}

impl CodeSource {
    /// Exact instruction ranges, in block order, before A20 and physical mapping.
    pub fn ranges(&self) -> impl Iterator<Item = Range<u64>> + '_ {
        self.source
            .bytes
            .spans
            .iter()
            .map(|span| u64::from(span.address)..u64::from(span.address) + span.bytes.len() as u64)
    }

    /// The same dependencies in shared backing storage, including guest aliases.
    pub fn backing_ranges(&self) -> impl Iterator<Item = Range<u64>> + '_ {
        self.source
            .bytes
            .spans
            .iter()
            .map(|span| u64::from(span.backing)..u64::from(span.backing) + span.bytes.len() as u64)
    }

    /// Reports writes recorded since registration, including data elsewhere on a page.
    /// Direct writes through `Memory::backing` bypass recording.
    pub fn has_recorded_writes(&self) -> bool {
        self.source
            .pages
            .iter()
            .any(|page| page.current.writes.load(Ordering::Acquire) != page.registered)
    }

    /// Validate and publish under the same lock as memory-view writes.
    pub(crate) fn publish_if_current(
        &self,
        memory: &Memory,
        publish: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let _registry = self.registry.pages.lock().unwrap();
        if self.has_recorded_writes() && !self.source.bytes.matches(&memory.backing) {
            return Err(Error::StaleCompilation);
        }
        publish()
    }

    pub(crate) fn belongs_to(&self, memory: &Memory) -> bool {
        Arc::ptr_eq(&self.registry, &memory.code)
    }
}

/// Rust-only metadata shared by every handle to one guest memory.
#[derive(Default)]
pub struct CodeRegistry {
    pages: Mutex<RegisteredPages>,
    registering: AsyncMutex<()>,
    revision: AtomicU64,
}

#[derive(Default)]
struct RegisteredPages {
    by_backing: BTreeMap<u32, Page>,
}

struct Page {
    generation: Weak<TrackedPage>,
    sources: Vec<Weak<Source>>,
}

struct TrackedPage {
    writes: AtomicU64,
    registry: Weak<CodeRegistry>,
}

impl Drop for TrackedPage {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            registry.revision.fetch_add(1, Ordering::Release);
        }
    }
}

impl RegisteredPages {
    fn wrote(&mut self, backing: u32, len: usize) {
        if len == 0 {
            return;
        }
        let last = (u64::from(backing) + len as u64 - 1) as u32;
        for (_, page) in self
            .by_backing
            .range((backing / PAGE_BYTES * PAGE_BYTES)..=(last / PAGE_BYTES * PAGE_BYTES))
        {
            if let Some(generation) = page.generation.upgrade() {
                generation.writes.fetch_add(1, Ordering::Release);
            }
        }
    }
}

impl Memory {
    pub(crate) fn code_reader(&self) -> CodeReader<'_> {
        CodeReader::new(self)
    }

    pub(crate) fn code_revision(&self) -> u64 {
        self.code.revision.load(Ordering::Acquire)
    }

    pub(crate) fn code_memory_map(&self) -> (u64, PhysicalMemoryMap) {
        let mut registry = self.code.pages.lock().unwrap();
        // A last owner can release a marker during this scan. Read the revision
        // first so that release still causes a later refresh.
        let revision = self.code_revision();
        let mut map = self.config.memory_map().clone();
        registry.by_backing.retain(|&backing, page| {
            let retained = page.generation.strong_count() != 0;
            if retained {
                map.set_code_page(backing, true);
            }
            retained
        });
        (revision, map)
    }

    /// Registered sources covering one backing byte, across pending results and
    /// CPU installations. Unused read-ahead and gaps between blocks are excluded.
    pub fn code_sources_at(&self, backing: u32) -> Vec<CodeSource> {
        let mut registry = self.code.pages.lock().unwrap();
        let Some(page) = registry
            .by_backing
            .get_mut(&(backing / PAGE_BYTES * PAGE_BYTES))
        else {
            return Vec::new();
        };
        page.sources.retain(|source| source.strong_count() != 0);
        page.sources
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|source| {
                source.bytes.spans.iter().any(|span| {
                    u64::from(backing) >= u64::from(span.backing)
                        && u64::from(backing) < u64::from(span.backing) + span.bytes.len() as u64
                })
            })
            .map(|source| CodeSource {
                registry: self.code.clone(),
                source,
            })
            .collect()
    }

    pub(super) fn write_storage(&self, backing: u32, bytes: &[u8]) {
        // Registration and host writes serialize through the same owner. The version
        // changes before releasing the lock, after all bytes have been written.
        let mut registry = self.code.pages.lock().unwrap();
        self.backing.write(backing, bytes);
        registry.wrote(backing, bytes.len());
    }
}

#[cfg(test)]
mod tests;
