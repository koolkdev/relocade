//! Installed guest instructions indexed by backing page.

use super::{page_base, CodeSource, Span};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// One installation of compiled guest code. Observers can retain its write history
/// after the CPU removes the module from dispatch.
pub struct InstalledCode {
    source: CodeSource,
    spans_by_page: BTreeMap<u32, Vec<usize>>,
    changed: AtomicBool,
}

impl InstalledCode {
    pub fn source(&self) -> &CodeSource {
        &self.source
    }

    /// Whether a managed write changed an instruction byte while this module was installed.
    pub fn was_modified(&self) -> bool {
        self.changed.load(Ordering::Acquire)
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

#[derive(Default)]
pub(crate) struct CodePages {
    by_backing: BTreeMap<u32, Vec<Arc<InstalledCode>>>,
}

impl CodePages {
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
            changed: AtomicBool::new(false),
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
    pub(super) fn mark_modified(&self, backing: u32, bytes: &[u8]) -> bool {
        let last = (u64::from(backing) + bytes.len() as u64 - 1) as u32;
        let mut changed = false;
        for (&page, codes) in self.by_backing.range(page_base(backing)..=page_base(last)) {
            for code in codes {
                if !code.was_modified()
                    && code
                        .spans_on(page)
                        .any(|span| span.changed_by(backing, bytes))
                {
                    code.changed.store(true, Ordering::Release);
                    changed = true;
                }
            }
        }
        changed
    }
}
