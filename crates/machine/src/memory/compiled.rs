//! Live compiled source belongs to backing storage, including all guest aliases.

mod policy;

use super::{code::CodeSource, resolve::Spans, Memory};
use relocade_x86::PhysicalMapping;
use std::ops::Range;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex, Weak},
};

#[derive(Default)]
pub struct CompiledMemory {
    watches: Vec<Weak<CodeWatch>>,
    revision: u64,
    policy: policy::Policy,
}

pub(crate) struct CodeWatch {
    sources: Arc<[CodeSource]>,
    changed: Mutex<BTreeSet<usize>>,
}

impl CodeWatch {
    pub(crate) fn changed(&self) -> bool {
        !self.changed.lock().unwrap().is_empty()
    }
}

impl Memory {
    pub(crate) fn excluded_code(&self) -> Vec<Range<u64>> {
        self.compiled.lock().unwrap().policy.excluded()
    }

    pub(crate) fn excludes_code(
        &self,
        address: u32,
        bytes: usize,
        excluded: &[Range<u64>],
    ) -> bool {
        Spans::new(self.config.memory_map(), address, bytes)
            .expect("decoded instruction range")
            .any(|span| {
                let backing = match span.mapping {
                    PhysicalMapping::Ram { backing_offset }
                    | PhysicalMapping::Rom { backing_offset } => u64::from(backing_offset),
                    _ => return false,
                };
                let index = excluded.partition_point(|range| range.end <= backing);
                excluded
                    .get(index)
                    .is_some_and(|range| range.start < backing + span.len as u64)
            })
    }

    pub(crate) fn watch_code(&self, sources: &Arc<[CodeSource]>) -> Arc<CodeWatch> {
        let watch = Arc::new(CodeWatch {
            sources: sources.clone(),
            changed: Mutex::default(),
        });
        let mut code = self.compiled.lock().unwrap();
        code.watches.push(Arc::downgrade(&watch));
        code.revision += 1;
        watch
    }

    pub(crate) fn code_pages(&self, revision: &mut u64) -> Option<BTreeSet<u32>> {
        let mut code = self.compiled.lock().unwrap();
        let before = code.watches.len();
        code.watches.retain(|watch| watch.strong_count() != 0);
        if before != code.watches.len() {
            code.revision += 1;
        }
        if *revision == code.revision {
            return None;
        }
        *revision = code.revision;
        Some(
            code.watches
                .iter()
                .filter_map(Weak::upgrade)
                .flat_map(|watch| {
                    watch
                        .sources
                        .iter()
                        .flat_map(|source| source.spans.iter())
                        .map(|span| span.backing >> 12)
                        .collect::<Vec<_>>()
                })
                .collect(),
        )
    }

    pub(super) fn changing_code(&self, offset: u32, bytes: &[u8]) {
        let start = u64::from(offset);
        let end = start + bytes.len() as u64;
        if start == end {
            return;
        }
        let mut code = self.compiled.lock().unwrap();
        let mut changed = Vec::new();
        for watch in code.watches.iter().filter_map(Weak::upgrade) {
            let mut blocks = watch.changed.lock().unwrap();
            for (index, source) in watch.sources.iter().enumerate() {
                if blocks.contains(&index) {
                    continue;
                }
                let differs = source.spans.iter().any(|span| {
                    let overlap_start = start.max(u64::from(span.backing));
                    let overlap_end = end.min(u64::from(span.backing) + span.bytes.len() as u64);
                    if overlap_start >= overlap_end {
                        return false;
                    }
                    let written =
                        &bytes[(overlap_start - start) as usize..(overlap_end - start) as usize];
                    let original = &span.bytes[(overlap_start - u64::from(span.backing)) as usize
                        ..(overlap_end - u64::from(span.backing)) as usize];
                    written != original
                });
                if differs {
                    blocks.insert(index);
                    changed.extend(source.spans.iter().map(|span| {
                        u64::from(span.backing)..u64::from(span.backing) + span.bytes.len() as u64
                    }));
                }
            }
        }
        if !changed.is_empty() {
            code.policy.invalidate(changed);
            self.code_state.atomic_store(0, 1_u8);
        }
    }
}
