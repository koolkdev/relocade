use super::{CapturedPage, Memory, PAGE_BYTES};
use futures_util::lock::Mutex;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

/// Immutable backing pages shared by all reads in one compilation.
pub(super) struct Pages<'a> {
    memory: &'a Memory,
    captured: RefCell<BTreeMap<u32, Rc<CapturedPage>>>,
    loading: Mutex<()>,
}

impl<'a> Pages<'a> {
    pub(super) fn new(memory: &'a Memory) -> Self {
        Self {
            memory,
            captured: RefCell::default(),
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
        let _loading = self.loading.lock().await;
        if let Some(page) = self.captured.borrow().get(&backing) {
            return page.clone();
        }
        let _paused = self.memory.execution.pause(|| {}).await;
        let page = Rc::new(
            self.memory
                .code
                .lock()
                .unwrap()
                .capture(&self.memory.backing, backing),
        );
        self.captured.borrow_mut().insert(backing, page.clone());
        page
    }

    pub(super) fn into_captured(self) -> BTreeMap<u32, Rc<CapturedPage>> {
        self.captured.into_inner()
    }
}

#[cfg(test)]
mod tests;
