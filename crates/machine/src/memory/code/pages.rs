use super::{CapturedPage, Memory, PAGE_BYTES};
use futures_lite::future::yield_now;
use futures_util::lock::Mutex;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

/// Immutable backing pages shared by all reads in one compilation.
pub(super) struct Pages<'a> {
    memory: &'a Memory,
    captured: RefCell<BTreeMap<u32, Rc<CapturedPage>>>,
    pending: RefCell<BTreeSet<u32>>,
    loading: Mutex<()>,
}

impl<'a> Pages<'a> {
    pub(super) fn new(memory: &'a Memory) -> Self {
        Self {
            memory,
            captured: RefCell::default(),
            pending: RefCell::default(),
            loading: Mutex::new(()),
        }
    }

    /// Returns the captured page containing this backing offset. Repeated reads
    /// share the same bytes and generation, even after the guest writes the page.
    pub(super) async fn get(&self, backing: u32) -> Rc<CapturedPage> {
        let backing = backing / PAGE_BYTES * PAGE_BYTES;
        if let Some(page) = self.captured.borrow().get(&backing) {
            return page.clone();
        }
        self.pending.borrow_mut().insert(backing);
        let _loading = self.loading.lock().await;
        if let Some(page) = self.captured.borrow().get(&backing) {
            return page.clone();
        }
        // Let other ready discoveries queue their pages before taking one CPU
        // pause for the batch. This yields within Rust, without a host timer.
        yield_now().await;
        self.capture_pending().await;
        self.captured.borrow()[&backing].clone()
    }

    async fn capture_pending(&self) {
        let _paused = self.memory.execution.pause(|| {}).await;
        // Keep requests queued while waiting, including if this read is dropped.
        for backing in self.pending.take() {
            let page = self
                .memory
                .code
                .lock()
                .unwrap()
                .capture(&self.memory.backing, backing);
            self.captured.borrow_mut().insert(backing, Rc::new(page));
        }
    }

    pub(super) fn into_captured(self) -> BTreeMap<u32, Rc<CapturedPage>> {
        self.captured.into_inner()
    }
}

#[cfg(test)]
mod tests;
