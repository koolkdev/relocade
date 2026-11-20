//! Installed guest instructions indexed by backing page.

use super::{page_base, CodeSource, Span};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

/// One installation of compiled guest code. Observers can retain its write history
/// after the CPU removes the module from dispatch.
pub struct InstalledCode {
    source: CodeSource,
    spans_by_page: BTreeMap<u32, Vec<usize>>,
    /// Zero until the first write invalidates this installation.
    invalidated_by: AtomicU64,
}

impl InstalledCode {
    pub fn source(&self) -> &CodeSource {
        &self.source
    }

    /// Whether a managed write changed an instruction byte while this module was installed.
    pub fn was_modified(&self) -> bool {
        self.invalidated_by.load(Ordering::Acquire) != 0
    }

    /// Each backing page belongs to one physical RAM or ROM page, including across A20 aliases.
    pub(crate) fn physical_pages(&self) -> impl Iterator<Item = u32> + '_ {
        self.spans_by_page
            .values()
            .map(|spans| self.source.spans[spans[0]].physical_page)
    }

    fn spans_on(&self, page: u32) -> impl Iterator<Item = &Span> {
        self.spans_by_page[&page]
            .iter()
            .map(|&index| &self.source.spans[index])
    }

    pub(crate) fn unregister(self: &Arc<Self>) {
        let Some(memory) = self.source.memory.upgrade() else {
            return;
        };
        let mut registry = memory.pages.lock().unwrap();
        for page in self.spans_by_page.keys() {
            let codes = registry.by_backing.get_mut(page).expect("installed page");
            codes.retain(|code| !Arc::ptr_eq(code, self));
            if codes.is_empty() {
                registry.by_backing.remove(page);
            }
        }
    }
}

/// Backing bytes that writes invalidated in two successive installations.
#[derive(Clone, Default)]
pub(super) struct ExcludedCode(Arc<BTreeSet<u32>>);

impl ExcludedCode {
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn intersects(&self, backing: u32, len: usize) -> bool {
        !self.is_empty()
            && self
                .0
                .range(backing..)
                .next()
                .is_some_and(|&byte| ((byte - backing) as usize) < len)
    }

    fn insert(&mut self, byte: u32) {
        if !self.0.contains(&byte) {
            Arc::make_mut(&mut self.0).insert(byte);
        }
    }
}

#[derive(Default)]
struct CodePatches {
    last_write: u64,
    /// Overlapping installations report the same byte with the same write ID.
    first_write: BTreeMap<u32, u64>,
    excluded: ExcludedCode,
}

impl CodePatches {
    fn record(&mut self, byte: u32, write_id: &mut u64) {
        if *write_id == 0 {
            self.last_write = self
                .last_write
                .checked_add(1)
                .expect("invalidating write ID exhausted");
            *write_id = self.last_write;
        }
        if *self.first_write.entry(byte).or_insert(*write_id) != *write_id {
            self.excluded.insert(byte);
        }
    }
}

#[derive(Default)]
pub(crate) struct CodePages {
    by_backing: BTreeMap<u32, Vec<Arc<InstalledCode>>>,
    patches: CodePatches,
}

impl CodePages {
    pub(super) fn excluded(&self) -> &ExcludedCode {
        &self.patches.excluded
    }

    pub(crate) fn register(&mut self, source: &CodeSource) -> Arc<InstalledCode> {
        let mut spans_by_page = BTreeMap::<_, Vec<_>>::new();
        for (index, span) in source.spans.iter().enumerate() {
            spans_by_page
                .entry(page_base(span.backing))
                .or_default()
                .push(index);
        }
        let code = Arc::new(InstalledCode {
            source: source.clone(),
            spans_by_page,
            invalidated_by: AtomicU64::new(0),
        });
        for &page in code.spans_by_page.keys() {
            self.by_backing.entry(page).or_default().push(code.clone());
        }
        code
    }

    pub(super) fn covering(&self, backing: u32) -> impl Iterator<Item = &Arc<InstalledCode>> {
        let page = page_base(backing);
        self.by_backing
            .get(&page)
            .into_iter()
            .flatten()
            .filter(move |code| {
                code.spans_on(page)
                    .any(|span| span.backing_range().contains(&u64::from(backing)))
            })
    }

    /// Returns true if this write newly marked at least one installation.
    pub(super) fn mark_modified(&mut self, backing: u32, bytes: &[u8], write_id: &mut u64) -> bool {
        let last = (u64::from(backing) + bytes.len() as u64 - 1) as u32;
        let mut changed = false;
        for (&page, codes) in self.by_backing.range(page_base(backing)..=page_base(last)) {
            for code in codes {
                let invalidated_by = code.invalidated_by.load(Ordering::Acquire);
                if invalidated_by != 0 && invalidated_by != *write_id {
                    continue;
                }
                let mut modified = false;
                for span in code.spans_on(page) {
                    modified |= span
                        .record_changes(backing, bytes, |byte| self.patches.record(byte, write_id));
                }
                if modified && invalidated_by == 0 {
                    code.invalidated_by.store(*write_id, Ordering::Release);
                    changed = true;
                }
            }
        }
        changed
    }
}
