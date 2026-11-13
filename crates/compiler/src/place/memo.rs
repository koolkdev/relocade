//! Indexed answers with copy-on-write pages for cheap path snapshots.

use std::{ops::Index, rc::Rc};

const PAGE_SIZE: usize = 64;

#[derive(Clone)]
pub(super) struct Memo<T: Copy> {
    // Dense value IDs give direct lookup; each path copies only pages it changes.
    pages: Vec<Option<Rc<[Option<T>; PAGE_SIZE]>>>,
}

#[cfg(test)]
mod tests {
    use super::Memo;

    #[test]
    fn a_fork_invalidates_a_suffix_without_changing_its_parent() {
        let mut parent = Memo::default();
        for (id, value) in [(0, 5), (63, 7), (64, 11), (65, 13), (130, 17)] {
            parent.insert(id, value);
        }
        let mut child = parent.clone();
        child.invalidate_from(64);
        assert_eq!(child.get(&63), Some(&7));
        assert_eq!(child.get(&64), None);
        assert_eq!(child.get(&130), None);
        child.insert(64, 19);
        assert_eq!(parent.get(&64), Some(&11));
        assert_eq!(parent.get(&130), Some(&17));
        let mut sibling = parent.clone();
        sibling.invalidate_from(65);
        assert_eq!(sibling.get(&64), Some(&11));
        assert_eq!(sibling.get(&65), None);
        assert_eq!(child.get(&64), Some(&19));
    }
}

impl<T: Copy> Default for Memo<T> {
    fn default() -> Self {
        Self { pages: Vec::new() }
    }
}

impl<T: Copy> Memo<T> {
    pub(super) fn get(&self, id: &usize) -> Option<&T> {
        self.pages.get(id / PAGE_SIZE)?.as_ref()?[id % PAGE_SIZE].as_ref()
    }

    pub(super) fn contains_key(&self, id: &usize) -> bool {
        self.get(id).is_some()
    }

    pub(super) fn insert(&mut self, id: usize, value: T) {
        let page = id / PAGE_SIZE;
        if self.pages.len() <= page {
            self.pages.resize_with(page + 1, || None);
        }
        let page = self.pages[page].get_or_insert_with(|| Rc::new([None; PAGE_SIZE]));
        Rc::make_mut(page)[id % PAGE_SIZE] = Some(value);
    }

    pub(super) fn remove(&mut self, id: usize) {
        if self.get(&id).is_some() {
            Rc::make_mut(self.pages[id / PAGE_SIZE].as_mut().unwrap())[id % PAGE_SIZE] = None;
        }
    }

    pub(super) fn invalidate_from(&mut self, id: usize) {
        let page = id / PAGE_SIZE;
        if page >= self.pages.len() {
            return;
        }
        if id % PAGE_SIZE == 0 {
            self.pages.truncate(page);
        } else {
            self.pages.truncate(page + 1);
            if let Some(page) = &mut self.pages[page] {
                Rc::make_mut(page)[id % PAGE_SIZE..].fill(None);
            }
        }
    }

    pub(super) fn clear(&mut self) {
        self.pages.clear();
    }
}

impl<T: Copy> Index<&usize> for Memo<T> {
    type Output = T;
    fn index(&self, id: &usize) -> &T {
        self.get(id).expect("the input has been inferred")
    }
}
