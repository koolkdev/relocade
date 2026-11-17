use super::{pages::Pages, CodeSource, Memory, Source, Span, PAGE_BYTES};
use crate::memory::{resolve::Spans, MemoryError};
use futures_util::future::join_all;
use relocade_x86::PhysicalMapping;
use std::{collections::BTreeSet, sync::Arc};

/// One copy per backing page, reused across discovery reads and aliases.
/// Only emitted instruction spans survive completion of the reader.
pub(crate) struct CodeReader<'a> {
    memory: &'a Memory,
    pages: Pages<'a>,
}

impl<'a> CodeReader<'a> {
    pub(super) fn new(memory: &'a Memory) -> Self {
        Self {
            memory,
            pages: Pages::new(memory),
        }
    }

    pub(crate) async fn read(
        &self,
        ranges: impl IntoIterator<Item = (u32, usize)>,
    ) -> Result<Vec<Vec<u8>>, MemoryError> {
        let mut reads = Vec::new();
        for (address, len) in ranges {
            for span in Spans::new(self.memory.config.memory_map(), address, len)? {
                if !matches!(
                    span.mapping,
                    PhysicalMapping::Ram { .. } | PhysicalMapping::Rom { .. }
                ) {
                    return Err(MemoryError::NotRamOrRom);
                }
            }
            reads.push(async move {
                let mut bytes = vec![0; len];
                assert_eq!(self.read_prefix(address, &mut bytes).await, len);
                bytes
            });
        }
        Ok(join_all(reads).await)
    }

    /// Copies captured RAM/ROM bytes across page boundaries. MMIO and holes end
    /// the readable prefix without accessing a device.
    pub(crate) async fn read_prefix(&self, address: u32, output: &mut [u8]) -> usize {
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
            let page = self.pages.get(backing).await;
            let offset = (backing % PAGE_BYTES) as usize;
            output[copied..copied + span.len]
                .copy_from_slice(&page.bytes[offset..offset + span.len]);
            copied += span.len;
        }
        copied
    }

    pub(crate) fn finish(self, blocks: impl IntoIterator<Item = (u32, usize)>) -> CodeSource {
        let pages = self.pages.into_captured();
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
                            bytes: pages[&page].bytes[offset..offset + span.len].into(),
                        };
                        linear = linear.wrapping_add(span.len as u32);
                        source
                    })
                    .collect()
            })
            .collect();
        let pages = pages
            .into_iter()
            .filter(|(page, _)| used.contains(page))
            .map(|(_, page)| page.version.clone())
            .collect();
        let source = Arc::new(Source { blocks, pages });
        self.memory.code.lock().unwrap().register(&source);
        CodeSource {
            registry: self.memory.code.clone(),
            source,
        }
    }
}
