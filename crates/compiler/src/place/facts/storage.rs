//! Sparse value-indexed snapshots. A fork shares its root; writes copy one radix path.

use std::{ops::Index, rc::Rc};

const LEVEL_BITS: u32 = 5;
const WIDTH: usize = 1 << LEVEL_BITS;
const MASK: usize = WIDTH - 1;

#[derive(Clone)]
enum Node<T: Clone> {
    Leaf(Rc<[Option<T>; WIDTH]>),
    Branch(Rc<[Option<Node<T>>; WIDTH]>),
}

#[derive(Clone)]
pub(super) struct Storage<T: Clone> {
    root: Option<Node<T>>,
    shift: u32,
}

impl<T: Clone> Default for Storage<T> {
    fn default() -> Self {
        Self {
            root: None,
            shift: 0,
        }
    }
}

fn fits(id: usize, shift: u32) -> bool {
    id.checked_shr(shift + LEVEL_BITS).unwrap_or(0) == 0
}

impl<T: Clone> Storage<T> {
    pub(super) fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Meet matching entries, skipping shared subtrees. `merge(a, a)` must keep `a`.
    pub(super) fn intersect(&mut self, other: &Self, merge: impl Fn(&T, &T) -> Option<T>) {
        while self.shift > other.shift {
            self.root = match self.root.take() {
                Some(Node::Branch(children)) => children[0].clone(),
                None => None,
                _ => unreachable!(),
            };
            self.shift -= LEVEL_BITS;
        }
        let mut incoming = other.root.as_ref();
        let mut shift = other.shift;
        while shift > self.shift {
            incoming = match incoming {
                Some(Node::Branch(children)) => children[0].as_ref(),
                None => None,
                _ => unreachable!(),
            };
            shift -= LEVEL_BITS;
        }
        intersect_node(&mut self.root, incoming, &merge);
        if self.root.is_none() {
            self.shift = 0;
        }
    }

    pub(super) fn get(&self, id: &usize) -> Option<&T> {
        if !fits(*id, self.shift) {
            return None;
        }
        let mut node = self.root.as_ref()?;
        let mut shift = self.shift;
        loop {
            match node {
                Node::Leaf(entries) => return entries[id & MASK].as_ref(),
                Node::Branch(children) => {
                    node = children[(id >> shift) & MASK].as_ref()?;
                    shift -= LEVEL_BITS;
                }
            }
        }
    }

    pub(super) fn contains_key(&self, id: &usize) -> bool {
        self.get(id).is_some()
    }

    fn slot(&mut self, id: usize) -> &mut Option<T> {
        while !fits(id, self.shift) {
            self.root = self.root.take().map(|root| {
                let mut children = std::array::from_fn(|_| None);
                children[0] = Some(root);
                Node::Branch(Rc::new(children))
            });
            self.shift += LEVEL_BITS;
        }
        let mut node = &mut self.root;
        let mut shift = self.shift;
        while shift != 0 {
            let Node::Branch(children) =
                node.get_or_insert_with(|| Node::Branch(Rc::new(std::array::from_fn(|_| None))))
            else {
                unreachable!()
            };
            node = &mut Rc::make_mut(children)[(id >> shift) & MASK];
            shift -= LEVEL_BITS;
        }
        let Node::Leaf(entries) =
            node.get_or_insert_with(|| Node::Leaf(Rc::new(std::array::from_fn(|_| None))))
        else {
            unreachable!()
        };
        &mut Rc::make_mut(entries)[id & MASK]
    }

    pub(super) fn insert(&mut self, id: usize, value: T) -> Option<T> {
        self.slot(id).replace(value)
    }

    pub(super) fn get_or_insert_default(&mut self, id: usize) -> &mut T
    where
        T: Default,
    {
        self.slot(id).get_or_insert_default()
    }

    pub(super) fn any(&self, mut predicate: impl FnMut(usize, &T) -> bool) -> bool {
        fn visit<T: Clone>(
            node: &Node<T>,
            base: usize,
            shift: u32,
            predicate: &mut impl FnMut(usize, &T) -> bool,
        ) -> bool {
            match node {
                Node::Leaf(entries) => entries.iter().enumerate().any(|(index, entry)| {
                    entry
                        .as_ref()
                        .is_some_and(|value| predicate(base | index, value))
                }),
                Node::Branch(children) => children.iter().enumerate().any(|(index, child)| {
                    child.as_ref().is_some_and(|child| {
                        visit(
                            child,
                            base | (index << shift),
                            shift - LEVEL_BITS,
                            predicate,
                        )
                    })
                }),
            }
        }
        self.root
            .as_ref()
            .is_some_and(|root| visit(root, 0, self.shift, &mut predicate))
    }

    pub(super) fn retain(&mut self, mut predicate: impl FnMut(&usize, &mut T) -> bool) {
        fn visit<T: Clone>(
            node: &mut Option<Node<T>>,
            base: usize,
            shift: u32,
            predicate: &mut impl FnMut(&usize, &mut T) -> bool,
        ) {
            let retained = match node.as_mut() {
                Some(Node::Leaf(entries)) => {
                    let entries = Rc::make_mut(entries);
                    for (index, entry) in entries.iter_mut().enumerate() {
                        if entry
                            .as_mut()
                            .is_some_and(|value| !predicate(&(base | index), value))
                        {
                            *entry = None;
                        }
                    }
                    entries.iter().any(Option::is_some)
                }
                Some(Node::Branch(children)) => {
                    let children = Rc::make_mut(children);
                    for (index, child) in children.iter_mut().enumerate() {
                        visit(
                            child,
                            base | (index << shift),
                            shift - LEVEL_BITS,
                            predicate,
                        );
                    }
                    children.iter().any(Option::is_some)
                }
                None => false,
            };
            if !retained {
                *node = None;
            }
        }
        visit(&mut self.root, 0, self.shift, &mut predicate);
        if self.root.is_none() {
            self.shift = 0;
        }
    }
}

fn intersect_node<T: Clone>(
    current: &mut Option<Node<T>>,
    other: Option<&Node<T>>,
    merge: &impl Fn(&T, &T) -> Option<T>,
) {
    let retained = match (current.as_mut(), other) {
        (Some(Node::Leaf(entries)), Some(Node::Leaf(incoming))) => {
            if Rc::ptr_eq(entries, incoming) {
                return;
            }
            let entries = Rc::make_mut(entries);
            for (entry, other) in entries.iter_mut().zip(incoming.iter()) {
                *entry = entry
                    .as_ref()
                    .zip(other.as_ref())
                    .and_then(|(a, b)| merge(a, b));
            }
            entries.iter().any(Option::is_some)
        }
        (Some(Node::Branch(children)), Some(Node::Branch(incoming))) => {
            if Rc::ptr_eq(children, incoming) {
                return;
            }
            let children = Rc::make_mut(children);
            for (child, other) in children.iter_mut().zip(incoming.iter()) {
                intersect_node(child, other.as_ref(), merge);
            }
            children.iter().any(Option::is_some)
        }
        (None, _) | (_, None) => false,
        _ => unreachable!(),
    };
    if !retained {
        *current = None;
    }
}

impl<T: Clone> Index<&usize> for Storage<T> {
    type Output = T;
    fn index(&self, id: &usize) -> &T {
        self.get(id)
            .expect("the value has an entry in this snapshot")
    }
}

#[cfg(test)]
mod tests {
    use super::Storage;

    #[test]
    fn sparse_updates_preserve_parent_and_siblings_across_radix_levels() {
        let mut parent = Storage::default();
        for id in [0, 31, 32, 1023, 1024, usize::MAX] {
            parent.insert(id, id);
        }
        let mut left = parent.clone();
        let right = parent.clone();
        left.insert(32, 7);
        left.insert(4096, 9);
        left.retain(|id, _| *id >= 32);
        assert_eq!(left.get(&0), None);
        assert_eq!(left.get(&32), Some(&7));
        assert_eq!(left.get(&4096), Some(&9));
        for original in [&parent, &right] {
            assert_eq!(original.get(&0), Some(&0));
            assert_eq!(original.get(&32), Some(&32));
            assert_eq!(original.get(&4096), None);
            assert_eq!(original.get(&usize::MAX), Some(&usize::MAX));
        }
        left.retain(|_, _| false);
        assert!(left.is_empty());
        left.insert(1, 2);
        assert_eq!(left.get(&1), Some(&2));
    }
}
