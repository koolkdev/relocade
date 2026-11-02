//! Host writes observed by installed code sharing this Memory handle.

use std::{collections::BTreeMap, sync::Mutex};

#[derive(Default)]
struct Page {
    users: usize,
    revision: u64,
}

#[derive(Default)]
pub(crate) struct CodePages(Mutex<BTreeMap<u32, Page>>);

impl CodePages {
    #[cfg(feature = "worker")]
    pub(crate) fn retain(&self, pages: impl IntoIterator<Item = u32>) -> BTreeMap<u32, u64> {
        let mut tracked = self.0.lock().unwrap();
        pages
            .into_iter()
            .map(|address| {
                let page = tracked.entry(address).or_default();
                page.users += 1;
                (address, page.revision)
            })
            .collect()
    }

    pub(crate) fn release(&self, pages: &BTreeMap<u32, u64>) {
        let mut tracked = self.0.lock().unwrap();
        for address in pages.keys() {
            let page = tracked.get_mut(address).expect("retained code page");
            page.users -= 1;
            if page.users == 0 {
                tracked.remove(address);
            }
        }
    }

    pub(crate) fn revisions(&self) -> BTreeMap<u32, u64> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|(&address, page)| (address, page.revision))
            .collect()
    }

    pub(crate) fn write(&self, offset: u32, len: usize) {
        if len == 0 {
            return;
        }
        let first = offset & !4095;
        let last = ((u64::from(offset) + len as u64 - 1) as u32) & !4095;
        for (_, page) in self.0.lock().unwrap().range_mut(first..=last) {
            page.revision = page
                .revision
                .checked_add(1)
                .expect("code page revision capacity");
        }
    }

    pub(crate) fn invalidate(&self) {
        for page in self.0.lock().unwrap().values_mut() {
            page.revision = page
                .revision
                .checked_add(1)
                .expect("code page revision capacity");
        }
    }
}
