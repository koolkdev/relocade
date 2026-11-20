//! Installed guest instructions indexed by backing page.

use super::{
    blacklist::{CodeBlacklist, ExcludedCode},
    page_base, CodeSource, ModuleId, Span,
};
use crate::runtime::CodeWake;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::Waker,
};

/// One installation of compiled guest code. Observers can retain its write history
/// after the CPU removes the module from dispatch.
pub struct InstalledCode {
    // Module IDs and wake registrations belong to the CPU that published the code.
    owner: Arc<CodeWake>,
    id: ModuleId,
    source: CodeSource,
    spans_by_page: BTreeMap<u32, Vec<usize>>,
    modified: AtomicBool,
}

impl InstalledCode {
    pub fn source(&self) -> &CodeSource {
        &self.source
    }

    /// Whether a managed write changed an instruction byte while this module was installed.
    pub fn was_modified(&self) -> bool {
        self.modified.load(Ordering::Acquire)
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
        if self.was_modified() {
            self.owner.forget_retirement(self.id);
        }
        for page in self.spans_by_page.keys() {
            let codes = registry.by_backing.get_mut(page).expect("installed page");
            codes.retain(|code| !Arc::ptr_eq(code, self));
            if codes.is_empty() {
                registry.by_backing.remove(page);
            }
        }
    }
}

/// Shared by all backing spans of one memory write, including page and segment crossings.
#[derive(Default)]
pub(in crate::memory) struct CodeWrite {
    invalidated: Vec<Arc<InstalledCode>>,
    // Aliases and overlapping installations can report the same backing byte.
    counted_bytes: BTreeSet<u32>,
}

impl CodeWrite {
    fn invalidated_here(&self, code: &Arc<InstalledCode>) -> bool {
        self.invalidated
            .iter()
            .any(|other| Arc::ptr_eq(other, code))
    }
}

#[derive(Default)]
pub(crate) struct CodePages {
    by_backing: BTreeMap<u32, Vec<Arc<InstalledCode>>>,
    blacklist: CodeBlacklist,
}

impl CodePages {
    pub(super) fn excluded(&self) -> &ExcludedCode {
        &self.blacklist.excluded
    }

    pub(crate) fn register(
        &mut self,
        owner: &Arc<CodeWake>,
        id: ModuleId,
        source: &CodeSource,
    ) -> Arc<InstalledCode> {
        let mut spans_by_page = BTreeMap::<_, Vec<_>>::new();
        for (index, span) in source.spans.iter().enumerate() {
            spans_by_page
                .entry(page_base(span.backing))
                .or_default()
                .push(index);
        }
        let code = Arc::new(InstalledCode {
            owner: owner.clone(),
            id,
            source: source.clone(),
            spans_by_page,
            modified: AtomicBool::new(false),
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

    /// Take run waiters here and wake them after unlocking the registry.
    pub(super) fn mark_modified(
        &mut self,
        backing: u32,
        bytes: &[u8],
        write: &mut CodeWrite,
    ) -> Vec<Waker> {
        let last = (u64::from(backing) + bytes.len() as u64 - 1) as u32;
        let mut waiters = Vec::new();
        for (&page, codes) in self.by_backing.range(page_base(backing)..=page_base(last)) {
            for code in codes {
                let was_modified = code.was_modified();
                // An unpacker can keep writing after invalidation. Only count the write
                // that made this installation stale, including any later spans of that write.
                if was_modified && !write.invalidated_here(code) {
                    continue;
                }
                let mut modified = false;
                for span in code.spans_on(page) {
                    modified |= span.record_changes(backing, bytes, |byte| {
                        if write.counted_bytes.insert(byte) {
                            self.blacklist.record_invalidation(byte);
                        }
                    });
                }
                if modified && !was_modified {
                    code.modified.store(true, Ordering::Release);
                    write.invalidated.push(code.clone());
                    if let Some(waiter) = code.owner.record_retirement(code.id) {
                        waiters.push(waiter);
                    }
                }
            }
        }
        waiters
    }
}
