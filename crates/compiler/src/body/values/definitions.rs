//! Definition locations stay aligned with SSA value allocation and compaction.
use super::{Phase, Value, ValueDefinition, ValueTable};
use crate::body::BlockId;
use std::num::NonZeroUsize;

#[derive(Default)]
pub(crate) struct Definitions {
    blocks: Vec<Option<NonZeroUsize>>,
}

impl Phase for Definitions {
    const INTERN_EXPRESSIONS: bool = false;

    fn record(&mut self, value: Value) {
        self.blocks.push(match value.definition {
            ValueDefinition::Parameter { block, .. }
            | ValueDefinition::RefinedAlias { block, .. } => NonZeroUsize::new(block.0 + 1),
            _ => None,
        });
    }

    fn compact(&mut self, retained: &[Option<usize>]) {
        let mut retained = retained.iter();
        self.blocks.retain(|_| retained.next().unwrap().is_some());
        self.blocks.shrink_to_fit();
    }
}

impl ValueTable<Definitions> {
    pub(crate) fn definition_block(&self, value: usize) -> Option<BlockId> {
        self.phase.blocks[value].map(|block| BlockId(block.get() - 1))
    }

    pub(in crate::body) fn place(&mut self, value: usize, block: BlockId) {
        self.phase.blocks[value] = NonZeroUsize::new(block.0 + 1);
    }
}
