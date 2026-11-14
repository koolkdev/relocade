//! Definition locations stay aligned with SSA value allocation and compaction.
use super::{Phase, Value, ValueDefinition, ValueTable};
use crate::body::BlockId;

#[derive(Default)]
pub(crate) struct Definitions {
    blocks: Vec<Option<BlockId>>,
}

impl Phase for Definitions {
    const INTERN_EXPRESSIONS: bool = false;

    fn record(&mut self, value: Value) {
        self.blocks.push(match value.definition {
            ValueDefinition::Parameter { block, .. }
            | ValueDefinition::RefinedAlias { block, .. } => Some(block),
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
        self.phase.blocks[value]
    }

    pub(in crate::body) fn place(&mut self, value: usize, block: BlockId) {
        self.phase.blocks[value] = Some(block);
    }
}
