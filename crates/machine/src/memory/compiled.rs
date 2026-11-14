//! Live compiled source belongs to backing storage, including all guest aliases.

use super::{code::CodeSource, Memory};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex, Weak},
};

#[derive(Default)]
pub struct CompiledMemory {
    watches: Vec<Weak<CodeWatch>>,
    revision: u64,
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
        let code = self.compiled.lock().unwrap();
        let mut changed = false;
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
                    changed = true;
                }
            }
        }
        if changed {
            self.code_state.atomic_store(0, 1_u8);
        }
    }
}
