//! Shared instruction sources and the backing pages retained by their snapshots.

mod pages;
mod reader;
pub(crate) use reader::CodeReader;

use super::Memory;
use relocade_wasm_runtime::{SharedMemory, WasmSharedMemory};
use relocade_x86::PhysicalMemoryMap;
use std::{
    collections::BTreeMap,
    ops::Range,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
};

const PAGE_BYTES: u32 = PhysicalMemoryMap::PAGE_BYTES;

/// Immutable instruction bytes used by a compilation. Clones retain the same
/// source registration, including while results are pending or installed on CPUs.
#[derive(Clone)]
pub struct CodeSource {
    registry: Arc<CodeRegistry>,
    source: Arc<Source>,
}

struct Source {
    blocks: Vec<Vec<Span>>,
    pages: Vec<PageVersion>,
}

struct Span {
    address: u32,
    backing: u32,
    bytes: Box<[u8]>,
}

#[derive(Clone)]
struct PageVersion {
    backing: u32,
    captured: u64,
    current: Arc<TrackedPage>,
}

struct CapturedPage {
    version: PageVersion,
    bytes: Box<[u8]>,
}

impl CapturedPage {
    // The registry lock covers both copying bytes and reading their generation.
    fn new(memory: &SharedMemory, backing: u32, current: Arc<TrackedPage>) -> Self {
        let captured = current.generation.load(Ordering::Acquire);
        let mut bytes = vec![0; PAGE_BYTES as usize];
        memory.read(backing, &mut bytes);
        Self {
            version: PageVersion {
                backing,
                captured,
                current,
            },
            bytes: bytes.into_boxed_slice(),
        }
    }
}

struct TrackedPage {
    generation: AtomicU64,
    registry: Weak<CodeRegistry>,
}

impl Drop for TrackedPage {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            registry.revision.fetch_add(1, Ordering::Release);
        }
    }
}

impl CodeSource {
    /// Exact instruction ranges, in block order, before A20 and physical mapping.
    pub fn ranges(&self) -> impl Iterator<Item = Range<u64>> + '_ {
        self.source
            .blocks
            .iter()
            .flatten()
            .map(|span| u64::from(span.address)..u64::from(span.address) + span.bytes.len() as u64)
    }

    /// The same dependencies in shared backing storage, including guest aliases.
    pub fn backing_ranges(&self) -> impl Iterator<Item = Range<u64>> + '_ {
        self.source
            .blocks
            .iter()
            .flatten()
            .map(|span| u64::from(span.backing)..u64::from(span.backing) + span.bytes.len() as u64)
    }

    /// Reports writes recorded since capture, including data elsewhere on a page.
    /// Admission and retirement of installed code remain the caller's responsibility.
    /// Direct writes through [`Memory::backing`] bypass recording.
    pub fn has_recorded_writes(&self) -> bool {
        self.source
            .pages
            .iter()
            .any(|page| page.current.generation.load(Ordering::Acquire) != page.captured)
    }

    pub(crate) fn belongs_to(&self, memory: &Memory) -> bool {
        Arc::ptr_eq(&self.registry, &memory.code)
    }
}

/// Rust-only metadata shared by every handle to one guest memory.
#[derive(Default)]
pub struct CodeRegistry {
    pages: Mutex<BTreeMap<u32, Page>>,
    revision: AtomicU64,
}

struct Page {
    generation: Weak<TrackedPage>,
    sources: Vec<Weak<Source>>,
}

impl CodeRegistry {
    fn capture_tracked(&self, memory: &SharedMemory, backing: u32) -> Option<CapturedPage> {
        let pages = self.pages.lock().unwrap();
        let current = pages.get(&backing)?.generation.upgrade()?;
        Some(CapturedPage::new(memory, backing, current))
    }

    // New registrations are published only after the memory owner pauses CPUs.
    fn capture(self: &Arc<Self>, memory: &SharedMemory, backing: u32) -> CapturedPage {
        let mut pages = self.pages.lock().unwrap();
        let page = pages.entry(backing).or_insert_with(|| Page {
            generation: Weak::new(),
            sources: Vec::new(),
        });
        let current = page.generation.upgrade().unwrap_or_else(|| {
            let current = Arc::new(TrackedPage {
                generation: AtomicU64::new(0),
                registry: Arc::downgrade(self),
            });
            page.generation = Arc::downgrade(&current);
            page.sources.clear();
            self.revision.fetch_add(1, Ordering::Release);
            current
        });
        CapturedPage::new(memory, backing, current)
    }

    fn register(&self, source: &Arc<Source>) {
        let mut pages = self.pages.lock().unwrap();
        for page in &source.pages {
            let sources = &mut pages.get_mut(&page.backing).expect("captured page").sources;
            sources.retain(|source| source.strong_count() != 0);
            sources.push(Arc::downgrade(source));
        }
    }

    fn write(&self, memory: &SharedMemory, backing: u32, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        // Capture and host writes serialize through the same owner. The version
        // changes before releasing the lock, after all bytes have been written.
        let pages = self.pages.lock().unwrap();
        memory.write(backing, bytes);
        let last = (u64::from(backing) + bytes.len() as u64 - 1) as u32;
        for (_, page) in
            pages.range((backing / PAGE_BYTES * PAGE_BYTES)..=(last / PAGE_BYTES * PAGE_BYTES))
        {
            if let Some(current) = page.generation.upgrade() {
                current.generation.fetch_add(1, Ordering::Release);
            }
        }
    }
}

impl Memory {
    /// Captures code pages in batches. First registrations pause machine CPUs;
    /// later captures of retained pages synchronize with their tracked writes.
    pub(crate) fn code_reader(&self) -> CodeReader<'_> {
        CodeReader::new(self)
    }

    pub(crate) fn code_revision(&self) -> u64 {
        self.code.revision.load(Ordering::Acquire)
    }

    pub(crate) fn code_memory_map(&self) -> (u64, PhysicalMemoryMap) {
        let revision = self.code_revision();
        let mut pages = self.code.pages.lock().unwrap();
        let mut map = self.config.memory_map().clone();
        pages.retain(|&backing, page| {
            if let Some(_retained) = page.generation.upgrade() {
                map.set_code_page(backing, true);
                true
            } else {
                false
            }
        });
        (revision, map)
    }

    /// Registered sources covering one backing byte, across pending results and
    /// CPU installations. Unused read-ahead and gaps between blocks are excluded.
    pub fn code_sources_at(&self, backing: u32) -> Vec<CodeSource> {
        let mut pages = self.code.pages.lock().unwrap();
        let Some(page) = pages.get_mut(&(backing / PAGE_BYTES * PAGE_BYTES)) else {
            return Vec::new();
        };
        page.sources.retain(|source| source.strong_count() != 0);
        page.sources
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|source| {
                source.blocks.iter().flatten().any(|span| {
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
        self.code.write(&self.backing, backing, bytes);
    }
}

#[cfg(test)]
mod tests;
