//! Persistent known bits indexed directly by compiler value IDs.
//! Forks share the root; updates copy only a branch path and its affected leaf.

use super::Bits;
use std::rc::Rc;

const LEVEL_BITS: u32 = 5;
const WIDTH: usize = 1 << LEVEL_BITS;
const INDEX_MASK: usize = WIDTH - 1;

#[cfg(test)]
mod tests;

#[derive(Clone, Default)]
pub(super) struct KnownBits {
    root: Option<Node>,
    // Bit offset used to select a child; zero means the root is a leaf.
    shift: u32,
}

#[derive(Clone)]
enum Node {
    Leaf(Rc<[Bits; WIDTH]>),
    Branch(Rc<[Option<Node>; WIDTH]>),
}

impl KnownBits {
    pub(super) fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    pub(super) fn get(&self, id: usize) -> Option<&Bits> {
        if !fits(id, self.shift) {
            return None;
        }
        let mut node = self.root.as_ref()?;
        let mut shift = self.shift;
        loop {
            match node {
                Node::Leaf(bits) => {
                    let bits = &bits[id & INDEX_MASK];
                    return (bits.mask != 0).then_some(bits);
                }
                Node::Branch(children) => {
                    node = children[(id >> shift) & INDEX_MASK].as_ref()?;
                    shift -= LEVEL_BITS;
                }
            }
        }
    }

    pub(super) fn insert(&mut self, id: usize, bits: Bits) {
        debug_assert_ne!(bits.mask, 0);
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
                unreachable!("directory levels contain branches")
            };
            node = &mut Rc::make_mut(children)[(id >> shift) & INDEX_MASK];
            shift -= LEVEL_BITS;
        }
        let Node::Leaf(entries) =
            node.get_or_insert_with(|| Node::Leaf(Rc::new([Bits::default(); WIDTH])))
        else {
            unreachable!("the final level contains facts")
        };
        Rc::make_mut(entries)[id & INDEX_MASK] = bits;
    }

    pub(super) fn retain_common(&mut self, other: &Self) {
        if self.is_empty() {
            return;
        }
        if other.is_empty() {
            self.root = None;
            self.shift = 0;
            return;
        }
        // The smaller root covers an initial interval of value IDs. Facts
        // outside that interval cannot be present in both incoming contexts.
        while self.shift > other.shift {
            self.root = match self.root.take() {
                Some(Node::Branch(children)) => children[0].clone(),
                None => None,
                Some(Node::Leaf(_)) => unreachable!("only branches can exceed a root's height"),
            };
            self.shift -= LEVEL_BITS;
        }
        let mut incoming = other.root.as_ref();
        let mut shift = other.shift;
        while shift > self.shift {
            incoming = match incoming {
                Some(Node::Branch(children)) => children[0].as_ref(),
                None => None,
                Some(Node::Leaf(_)) => unreachable!("only branches can exceed a root's height"),
            };
            shift -= LEVEL_BITS;
        }
        retain_node(&mut self.root, incoming);
        if self.root.is_none() {
            self.shift = 0;
        }
    }

    pub(super) fn conflicts_with(&self, mut bits_at: impl FnMut(usize) -> Bits) -> bool {
        self.root
            .as_ref()
            .is_some_and(|root| root.conflicts_with(0, self.shift, &mut bits_at))
    }
}

fn fits(id: usize, shift: u32) -> bool {
    id.checked_shr(shift + LEVEL_BITS).unwrap_or(0) == 0
}

fn retain_node(current: &mut Option<Node>, incoming: Option<&Node>) {
    let retained = match (current.as_mut(), incoming) {
        (Some(Node::Leaf(bits)), Some(Node::Leaf(other))) => {
            if Rc::ptr_eq(bits, other) {
                return;
            }
            let common = std::array::from_fn(|index| bits[index].common(other[index]));
            let retained = common.iter().any(|bits| bits.mask != 0);
            if retained && bits.as_ref() != &common {
                *Rc::make_mut(bits) = common;
            }
            retained
        }
        (Some(Node::Branch(children)), Some(Node::Branch(other))) => {
            if Rc::ptr_eq(children, other) {
                return;
            }
            let children = Rc::make_mut(children);
            for (child, incoming) in children.iter_mut().zip(other.iter()) {
                retain_node(child, incoming.as_ref());
            }
            children.iter().any(Option::is_some)
        }
        (None, _) | (_, None) => false,
        _ => unreachable!("aligned roots have matching levels"),
    };
    if !retained {
        *current = None;
    }
}

impl Node {
    fn conflicts_with(
        &self,
        base: usize,
        shift: u32,
        bits_at: &mut impl FnMut(usize) -> Bits,
    ) -> bool {
        match self {
            Self::Leaf(bits) => bits
                .iter()
                .enumerate()
                .any(|(index, &bits)| bits.mask != 0 && bits.conflicts(bits_at(base | index))),
            Self::Branch(children) => children.iter().enumerate().any(|(index, child)| {
                child.as_ref().is_some_and(|child| {
                    child.conflicts_with(base | (index << shift), shift - LEVEL_BITS, bits_at)
                })
            }),
        }
    }
}
