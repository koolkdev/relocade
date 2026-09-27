//! Indexed block ancestry and authored operations for compiler passes.

use std::collections::HashMap;

use super::{Block, Invocation, Operation, Site};
use crate::memory::Location;

struct BlockInfo<'a> {
    block: &'a Block,
    parent: Option<Site>,
    depth: usize,
}

pub(crate) struct BlockTree<'a>(HashMap<usize, BlockInfo<'a>>);

impl<'a> BlockTree<'a> {
    pub(crate) fn new(root: &'a Block) -> Self {
        let mut blocks = HashMap::new();
        let mut pending = vec![(root, None, 0)];
        while let Some((block, parent, depth)) = pending.pop() {
            for (index, operation) in block.operations.iter().enumerate() {
                for child in operation.children() {
                    pending.push((
                        child,
                        Some(Site {
                            block: block.id,
                            index,
                        }),
                        depth + 1,
                    ));
                }
            }
            blocks.insert(
                block.id,
                BlockInfo {
                    block,
                    parent,
                    depth,
                },
            );
        }
        Self(blocks)
    }

    pub(crate) fn block(&self, id: usize) -> &'a Block {
        self.0[&id].block
    }

    pub(crate) fn parent(&self, id: usize) -> Option<Site> {
        self.0[&id].parent
    }

    // Detached construction values can still name a discarded operation.
    pub(crate) fn operation(&self, site: Site) -> Option<&'a Operation> {
        self.0
            .get(&site.block)
            .map(|info| &info.block.operations[site.index])
    }

    pub(crate) fn load_location(&self, site: Site) -> Location {
        let Some(Operation::Load { location }) = self.operation(site) else {
            unreachable!("a load result names an attached load")
        };
        *location
    }

    pub(crate) fn call(&self, site: Site) -> (&'a Invocation, &'a [usize]) {
        let Some(Operation::Call {
            invocation,
            outputs,
        }) = self.operation(site)
        else {
            unreachable!("a call result names an attached invocation")
        };
        (invocation, outputs)
    }

    /// Lift both sites into their nearest common block, preserving input order.
    pub(crate) fn common_block(&self, mut a: Site, mut b: Site) -> (Site, Site) {
        while self.0[&a.block].depth > self.0[&b.block].depth {
            a = self.parent(a.block).expect("a deeper block has a parent");
        }
        while self.0[&b.block].depth > self.0[&a.block].depth {
            b = self.parent(b.block).expect("a deeper block has a parent");
        }
        while a.block != b.block {
            a = self.parent(a.block).expect("distinct blocks have parents");
            b = self.parent(b.block).expect("distinct blocks have parents");
        }
        (a, b)
    }
}
