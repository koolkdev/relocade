//! Construction queries share scalar transfers with SSA without analyzing control flow.
use super::{Construction, FunctionArena};
use crate::{
    body::{infer_bits, Bits, BlockId, ValueDefinition},
    literal::Literal,
    BuildError,
};

impl FunctionArena {
    pub(crate) fn assume_bits(
        &self,
        source: usize,
        block: BlockId,
        mask: u64,
        expected: u64,
    ) -> Result<usize, BuildError> {
        let mut arena = self.0.borrow_mut();
        let arena = arena.as_mut().ok_or(BuildError::BodyClosed)?;
        let ty = arena.graph.values[source].ty;
        let mut known = Bits::constant(expected, mask & ty.mask());
        let previous = arena.known_bits(source);
        if previous.conflicts(known) {
            // Construction also visits continuations of unconditional exits.
            // As in SSA refinement, impossible facts add no knowledge there.
            known = Bits::default();
        }
        if previous.union(known) == previous {
            return Ok(source);
        }
        let value = arena.graph.values.refined_alias(source, block, known);
        arena
            .facts
            .as_mut()
            .unwrap()
            .insert(value, previous.union(known));
        Ok(value)
    }

    pub(crate) fn constant_literal(&self, value: usize) -> Result<Option<Literal>, BuildError> {
        let mut arena = self.0.borrow_mut();
        let arena = arena.as_mut().ok_or(BuildError::BodyClosed)?;
        if let ValueDefinition::Literal(bits) = arena.graph.values[value].definition {
            return Ok(Some(bits));
        }
        if arena.facts.is_none() {
            return Ok(None);
        }
        let mask = arena.graph.values[value].ty.mask();
        Ok(arena.known_bits(value).known(mask).map(Literal::from))
    }
}

impl Construction {
    fn known_bits(&mut self, root: usize) -> Bits {
        let facts = self.facts.get_or_insert_with(Default::default);
        if let Some(&bits) = facts.get(&root) {
            return bits;
        }
        let values = &self.graph.values;
        let mut work = vec![root];
        while let Some(id) = work.pop() {
            if facts.contains_key(&id) {
                continue;
            }
            let pending = work.len();
            work.push(id);
            // Follow only dependencies the scalar transfer actually reads.
            // Float/vector calculations and unsupported transfers supply their
            // initial bounds without walking their expression graphs.
            let bits = infer_bits(values, id, |input| {
                facts.get(&input).copied().unwrap_or_else(|| {
                    work.push(input);
                    Bits::default()
                })
            });
            if work.len() == pending + 1 {
                work.pop();
                facts.insert(id, bits);
            }
        }
        facts[&root]
    }
}
