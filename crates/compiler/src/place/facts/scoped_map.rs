//! Sparse facts with undo history for nested path scopes.
use rustc_hash::FxHashMap;
use std::{hash::Hash, ops::Deref};

#[cfg(test)]
mod tests;

pub(super) struct ScopedMap<K, V> {
    entries: FxHashMap<K, V>,
    changes: Vec<(K, Option<V>)>,
    scopes: usize,
}

/// Checkpoints belong to one map and must be restored in reverse creation order.
pub(super) struct Checkpoint(usize);

impl<K, V> Default for ScopedMap<K, V> {
    fn default() -> Self {
        Self {
            entries: FxHashMap::default(),
            changes: Vec::new(),
            scopes: 0,
        }
    }
}

impl<K: Clone, V: Clone> Clone for ScopedMap<K, V> {
    /// A snapshot owns its current entries, without the original's undo history.
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            ..Self::default()
        }
    }
}

// Read access cannot bypass the mutation log.
impl<K, V> Deref for ScopedMap<K, V> {
    type Target = FxHashMap<K, V>;

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

impl<K: Copy + Eq + Hash, V: Copy + Eq> ScopedMap<K, V> {
    pub(super) fn checkpoint(&mut self) -> Checkpoint {
        self.scopes += 1;
        Checkpoint(self.changes.len())
    }

    pub(super) fn restore(&mut self, checkpoint: Checkpoint) {
        for (key, previous) in self.changes.drain(checkpoint.0..).rev() {
            match previous {
                Some(value) => {
                    self.entries.insert(key, value);
                }
                None => {
                    self.entries.remove(&key);
                }
            }
        }
        self.scopes -= 1;
    }

    pub(super) fn insert(&mut self, key: K, value: V) -> Option<V> {
        let previous = self.entries.insert(key, value);
        if self.scopes != 0 && previous != Some(value) {
            self.changes.push((key, previous));
        }
        previous
    }

    pub(super) fn retain(&mut self, mut keep: impl FnMut(&K, &mut V) -> bool) {
        self.entries.retain(|key, value| {
            let previous = *value;
            let retained = keep(key, value);
            if self.scopes != 0 && (!retained || previous != *value) {
                self.changes.push((*key, Some(previous)));
            }
            retained
        });
    }
}
