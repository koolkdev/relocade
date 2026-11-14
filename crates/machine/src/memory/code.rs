//! Instruction reads retain the bytes that discovery actually decodes.

use super::{resolve::Spans, Memory, MemoryError};
use relocade_wasm_runtime::WasmSharedMemory;
use relocade_x86::PhysicalMapping;
use std::collections::BTreeMap;

const PAGE_BYTES: u32 = relocade_x86::PhysicalMemoryMap::PAGE_BYTES;

#[derive(Clone)]
pub(crate) struct CodeSource {
    pub(crate) spans: Vec<CodeSpan>,
}

#[derive(Clone)]
pub(crate) struct CodeSpan {
    pub(crate) address: u32,
    pub(crate) backing: u32,
    pub(crate) bytes: Box<[u8]>,
}

impl CodeSource {
    /// Installation runs between guest entries under the single-writer contract.
    pub(crate) fn matches(&self, memory: &Memory) -> bool {
        self.spans.iter().all(|span| {
            let mapping = memory.configuration().memory_map().get(span.address);
            if !matches!(mapping,
                PhysicalMapping::Ram { backing_offset } | PhysicalMapping::Rom { backing_offset }
                    if backing_offset == span.backing)
            {
                return false;
            }
            let mut current = vec![0; span.bytes.len()];
            memory.backing().read(span.backing, &mut current);
            current == *span.bytes
        })
    }
}

/// Pages are copied on first use. Aliases and repeated discovery reads use the
/// same copy; only emitted instruction spans become installation dependencies.
pub(crate) struct CodeReader<'a> {
    memory: &'a Memory,
    pages: BTreeMap<u32, Box<[u8]>>,
}

impl Memory {
    pub(crate) fn code_reader(&self) -> CodeReader<'_> {
        CodeReader {
            memory: self,
            pages: BTreeMap::new(),
        }
    }
}

impl CodeReader<'_> {
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
            let bytes = self.pages.entry(page).or_insert_with(|| {
                let mut bytes = vec![0; PAGE_BYTES as usize];
                self.memory.backing.read(page, &mut bytes);
                bytes.into_boxed_slice()
            });
            let offset = (backing - page) as usize;
            output[copied..copied + span.len].copy_from_slice(&bytes[offset..offset + span.len]);
            copied += span.len;
        }
        copied
    }

    pub(crate) fn source(&self, address: u32, len: usize) -> CodeSource {
        let mut linear = address;
        let spans = Spans::new(self.memory.config.memory_map(), address, len)
            .expect("decoded instruction range")
            .map(|span| {
                let backing = match span.mapping {
                    PhysicalMapping::Ram { backing_offset }
                    | PhysicalMapping::Rom { backing_offset } => backing_offset,
                    _ => unreachable!("decoded instructions came from backing"),
                };
                let page = backing / PAGE_BYTES * PAGE_BYTES;
                let offset = (backing - page) as usize;
                let source = CodeSpan {
                    address: linear,
                    backing,
                    bytes: self.pages[&page][offset..offset + span.len].into(),
                };
                linear = linear.wrapping_add(span.len as u32);
                source
            })
            .collect();
        CodeSource { spans }
    }
}
