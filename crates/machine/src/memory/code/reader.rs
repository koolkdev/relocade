use super::{CodeSource, Memory, Span, PAGE_BYTES};
use crate::memory::{
    resolve::{linear_address, segment_remaining, Spans},
    MemoryError,
};
use relocade_wasm_runtime::WasmSharedMemory;
use relocade_x86::{PhysicalMapping, SegmentRegister};
use std::{collections::BTreeMap, sync::Arc};

/// One copy per backing page, reused across discovery reads and aliases.
/// Only emitted instruction spans survive completion of the reader.
pub(crate) struct CodeReader<'a> {
    memory: &'a Memory,
    pages: BTreeMap<u32, Box<[u8]>>,
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

    /// Captures the readable RAM/ROM prefix within a code segment, without offset wrap.
    pub(crate) fn read_segment_prefix(
        &mut self,
        segment: &SegmentRegister,
        offset: u32,
        output: &mut [u8],
    ) -> usize {
        let Ok(remaining) = segment_remaining(segment, offset) else {
            return 0;
        };
        let Ok(address) = linear_address(segment, offset) else {
            return 0;
        };
        let len = remaining.min(output.len() as u64) as usize;
        self.read_prefix(address, &mut output[..len])
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
            let page = super::page_base(backing);
            let snapshot = self.pages.entry(page).or_insert_with(|| {
                let mut bytes = vec![0; PAGE_BYTES as usize];
                let _pages = self.memory.code.lock().unwrap();
                self.memory.backing.read(page, &mut bytes);
                bytes.into_boxed_slice()
            });
            let offset = (backing - page) as usize;
            output[copied..copied + span.len].copy_from_slice(&snapshot[offset..offset + span.len]);
            copied += span.len;
        }
        copied
    }

    pub(crate) fn finish(self, blocks: impl IntoIterator<Item = (u32, usize)>) -> CodeSource {
        let mut spans = Vec::new();
        for (address, len) in blocks {
            let mut linear = address;
            for span in Spans::new(self.memory.config.memory_map(), address, len)
                .expect("decoded instruction range")
            {
                let backing = match span.mapping {
                    PhysicalMapping::Ram { backing_offset }
                    | PhysicalMapping::Rom { backing_offset } => backing_offset,
                    _ => unreachable!("decoded instructions came from backing"),
                };
                let page = super::page_base(backing);
                let offset = (backing - page) as usize;
                spans.push(Span {
                    address: linear,
                    backing,
                    bytes: self.pages[&page][offset..offset + span.len].into(),
                });
                linear = linear.wrapping_add(span.len as u32);
            }
        }
        CodeSource {
            memory: Arc::downgrade(&self.memory.code),
            spans: spans.into(),
        }
    }
}
