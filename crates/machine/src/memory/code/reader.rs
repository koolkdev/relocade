use super::{CodeSource, Memory, PageVersion, Source, Span, PAGE_BYTES};
use crate::memory::{resolve::Spans, MemoryError};
use relocade_wasm_runtime::WasmSharedMemory;
use relocade_x86::PhysicalMapping;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{atomic::Ordering, Arc},
};

struct Page {
    version: PageVersion,
    bytes: Box<[u8]>,
}

/// One copy per backing page, reused across discovery reads and aliases.
/// Only emitted instruction spans survive completion of the reader.
pub(crate) struct CodeReader<'a> {
    memory: &'a Memory,
    pages: BTreeMap<u32, Page>,
}

impl<'a> CodeReader<'a> {
    pub(super) fn new(memory: &'a Memory) -> Self {
        Self {
            memory,
            pages: BTreeMap::new(),
        }
    }

    pub(crate) fn read(&mut self, address: u32, len: usize) -> Result<Vec<u8>, MemoryError> {
        for span in Spans::new(self.memory.config.memory_map(), address, len)? {
            if !matches!(
                span.mapping,
                PhysicalMapping::Ram { .. } | PhysicalMapping::Rom { .. }
            ) {
                return Err(MemoryError::NotRamOrRom);
            }
        }
        let mut bytes = vec![0; len];
        self.read_prefix(address, &mut bytes);
        Ok(bytes)
    }

    pub(crate) fn read_prefix(&mut self, address: u32, output: &mut [u8]) -> usize {
        let len = (output.len() as u64).min((1_u64 << 32) - u64::from(address)) as usize;
        let spans = Spans::new(self.memory.config.memory_map(), address, len)
            .expect("the prefix fits the guest address space");
        let mut copied = 0;
        for span in spans {
            let backing = match span.mapping {
                PhysicalMapping::Ram { backing_offset }
                | PhysicalMapping::Rom { backing_offset } => backing_offset,
                _ => break,
            };
            let page = backing / PAGE_BYTES * PAGE_BYTES;
            let snapshot = self.pages.entry(page).or_insert_with(|| {
                let mut registry = self.memory.code.lock().unwrap();
                let current = registry.retain_page(page);
                let captured = current.load(Ordering::Acquire);
                let mut bytes = vec![0; PAGE_BYTES as usize];
                self.memory.backing.read(page, &mut bytes);
                Page {
                    version: PageVersion {
                        backing: page,
                        captured,
                        current,
                    },
                    bytes: bytes.into_boxed_slice(),
                }
            });
            let offset = (backing - page) as usize;
            output[copied..copied + span.len]
                .copy_from_slice(&snapshot.bytes[offset..offset + span.len]);
            copied += span.len;
        }
        copied
    }

    pub(crate) fn finish(self, blocks: impl IntoIterator<Item = (u32, usize)>) -> CodeSource {
        let mut used = BTreeSet::new();
        let blocks = blocks
            .into_iter()
            .map(|(address, len)| {
                let mut linear = address;
                Spans::new(self.memory.config.memory_map(), address, len)
                    .expect("decoded instruction range")
                    .map(|span| {
                        let backing = match span.mapping {
                            PhysicalMapping::Ram { backing_offset }
                            | PhysicalMapping::Rom { backing_offset } => backing_offset,
                            _ => unreachable!("decoded instructions came from backing"),
                        };
                        let page = backing / PAGE_BYTES * PAGE_BYTES;
                        used.insert(page);
                        let offset = (backing - page) as usize;
                        let source = Span {
                            address: linear,
                            backing,
                            bytes: self.pages[&page].bytes[offset..offset + span.len].into(),
                        };
                        linear = linear.wrapping_add(span.len as u32);
                        source
                    })
                    .collect()
            })
            .collect();
        let pages = self
            .pages
            .into_iter()
            .filter(|(page, _)| used.contains(page))
            .map(|(_, page)| page.version)
            .collect();
        let source = Arc::new(Source { blocks, pages });
        self.memory.code.lock().unwrap().register(&source);
        CodeSource {
            registry: self.memory.code.clone(),
            source,
        }
    }
}
