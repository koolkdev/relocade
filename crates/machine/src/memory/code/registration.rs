//! Register the emitted instruction bytes as one unit before delivering a module.

use super::{
    CodeRegistry, CodeSource, InstructionBytes, Memory, Page, PageVersion, RegisteredPages, Source,
    TrackedPage,
};
use crate::{Error, Result};
use relocade_wasm_runtime::SharedMemory;
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

impl Memory {
    /// Register an entire module together. Retain existing markers while checking
    /// whether any page needs its first stopped registration.
    pub(crate) async fn register_code(&self, bytes: InstructionBytes) -> Result<CodeSource> {
        let keys = bytes.pages();
        let pages = self.code.retain_pages(&keys);
        if pages.iter().all(|page| page.tracked.is_some()) {
            return self.code.register(&self.backing, bytes, pages);
        }
        // Only first registrations serialize. Another job may have registered
        // these pages while we waited, so check again before stopping CPUs.
        let _first = self.code.registering.lock().await;
        let pages = self.code.retain_pages(&keys);
        let _paused = if pages.iter().any(|page| page.tracked.is_none()) {
            Some(self.execution.pause(|| {}).await)
        } else {
            None
        };
        self.code.register(&self.backing, bytes, pages)
    }
}

/// Pins an existing marker until this registration owns it; absent markers need a pause.
struct BackingPage {
    backing: u32,
    tracked: Option<Arc<TrackedPage>>,
}

impl CodeRegistry {
    fn retain_pages(&self, keys: &BTreeSet<u32>) -> Vec<BackingPage> {
        let pages = self.pages.lock().unwrap();
        keys.iter()
            .map(|&backing| BackingPage {
                backing,
                tracked: pages
                    .by_backing
                    .get(&backing)
                    .and_then(|page| page.generation.upgrade()),
            })
            .collect()
    }

    fn register(
        self: &Arc<Self>,
        backing: &SharedMemory,
        bytes: InstructionBytes,
        pages: Vec<BackingPage>,
    ) -> Result<CodeSource> {
        let mut registry = self.pages.lock().unwrap();
        if !bytes.matches(backing) {
            return Err(Error::StaleCompilation);
        }
        let added_pages = pages.iter().any(|page| page.tracked.is_none());
        let versions = pages
            .into_iter()
            .map(|page| {
                let current = page
                    .tracked
                    .unwrap_or_else(|| registry.start_tracking(page.backing, self));
                PageVersion {
                    backing: page.backing,
                    registered: current.writes.load(Ordering::Acquire),
                    current,
                }
            })
            .collect();
        let source = Arc::new(Source {
            bytes,
            pages: versions,
        });
        registry.add_source(&source);
        if added_pages {
            self.revision.fetch_add(1, Ordering::Release);
        }
        Ok(CodeSource {
            registry: self.clone(),
            source,
        })
    }
}

impl RegisteredPages {
    fn start_tracking(&mut self, backing: u32, registry: &Arc<CodeRegistry>) -> Arc<TrackedPage> {
        let current = Arc::new(TrackedPage {
            writes: AtomicU64::new(0),
            registry: Arc::downgrade(registry),
        });
        self.by_backing.insert(
            backing,
            Page {
                generation: Arc::downgrade(&current),
                sources: Vec::new(),
            },
        );
        current
    }

    fn add_source(&mut self, source: &Arc<Source>) {
        for page in &source.pages {
            let sources = &mut self
                .by_backing
                .get_mut(&page.backing)
                .expect("registered page")
                .sources;
            sources.retain(|source| source.strong_count() != 0);
            sources.push(Arc::downgrade(source));
        }
    }
}
