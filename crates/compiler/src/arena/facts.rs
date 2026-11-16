//! Construction queries share scalar transfers with SSA without analyzing control flow.
use super::{Construction, FunctionArena};
use crate::{
    body::{infer_bits, initial_bits, Bits, BlockId, ValueDefinition, ValueTable},
    literal::Literal,
    BuildError,
};
use rustc_hash::FxHashMap;

#[derive(Clone, Copy)]
pub(super) struct CachedBits {
    bits: Bits,
    // Every input used by the transfer is resolved; remaining bits stay unknown.
    complete: bool,
}

impl CachedBits {
    fn infer(
        values: &ValueTable,
        facts: &FxHashMap<usize, Self>,
        value: usize,
        mut unresolved: impl FnMut(usize),
    ) -> Self {
        let mut complete = true;
        let bits = infer_bits(values, value, |input| {
            let fact = facts.get(&input).copied().unwrap_or(Self {
                bits: initial_bits(values, input),
                complete: false,
            });
            if !fact.sufficient(values[input].ty.carrier().mask()) {
                complete = false;
                unresolved(input);
            }
            fact.bits
        });
        Self { bits, complete }
    }

    fn sufficient(self, mask: u64) -> bool {
        self.complete || self.bits.known(mask).is_some()
    }
}

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
        let previous = arena.known_bits(source, mask & ty.mask());
        if previous.conflicts(known) {
            // Construction also visits continuations of unconditional exits.
            // As in SSA refinement, impossible facts add no knowledge there.
            known = Bits::default();
        }
        if previous.union(known) == previous
            && (known == Bits::default()
                || arena.graph.values.depends_on_assumption(source)
                || matches!(
                    arena.graph.values[source].definition,
                    ValueDefinition::Literal(_)
                ))
        {
            return Ok(source);
        }
        let value = arena.graph.values.refined_alias(source, block, known);
        let facts = arena.facts.as_mut().unwrap();
        let complete = facts[&source].complete;
        facts.insert(
            value,
            CachedBits {
                bits: previous.union(known),
                complete,
            },
        );
        Ok(value)
    }

    pub(crate) fn constant_literal(&self, value: usize) -> Result<Option<Literal>, BuildError> {
        let mut arena = self.0.borrow_mut();
        let arena = arena.as_mut().ok_or(BuildError::BodyClosed)?;
        if let ValueDefinition::Literal(bits) = arena.graph.values[value].definition {
            return Ok(Some(bits));
        }
        if !arena.graph.values.depends_on_assumption(value) {
            return Ok(None);
        }
        let mask = arena.graph.values[value].ty.mask();
        Ok(arena.known_bits(value, mask).known(mask).map(Literal::from))
    }
}

impl Construction {
    /// Use already-available input facts in ordinary construction folding. This
    /// never walks another recipe or discards unknown physical carrier bits.
    pub(super) fn fold_assumed_constant(&mut self, value: usize, first: usize) -> usize {
        if !self.graph.values.depends_on_assumption(value) {
            return value;
        }
        // Normalization can introduce intermediate scalar recipes. Their input
        // facts become available in allocation order, without revisiting a graph.
        for id in first..self.graph.values.len() {
            self.cache_calculation(id);
        }
        if value < first {
            self.cache_calculation(value);
        }
        let ty = self.graph.values[value].ty;
        if !ty.is_integer() {
            return value;
        }
        let bits = self
            .facts
            .as_ref()
            .and_then(|facts| facts.get(&value))
            .and_then(|fact| fact.bits.known(ty.carrier().mask()));
        match bits {
            Some(bits) => self.graph.values.carrier_literal(ty, bits),
            None => value,
        }
    }

    fn cache_calculation(&mut self, value: usize) {
        let ty = self.graph.values[value].ty;
        if !ty.is_integer() || !self.graph.values.depends_on_assumption(value) {
            return;
        }
        let facts = self.facts.get_or_insert_with(Default::default);
        if !facts.get(&value).is_some_and(|fact| fact.complete) {
            let fact = CachedBits::infer(&self.graph.values, facts, value, |_| {});
            facts.insert(value, fact);
        }
    }

    fn known_bits(&mut self, root: usize, mask: u64) -> Bits {
        let facts = self.facts.get_or_insert_with(Default::default);
        if let Some(fact) = facts.get(&root).filter(|fact| fact.sufficient(mask)) {
            return fact.bits;
        }
        let values = &self.graph.values;
        let mut work = vec![root];
        while let Some(id) = work.pop() {
            let requested = if id == root {
                mask
            } else {
                values[id].ty.carrier().mask()
            };
            if facts
                .get(&id)
                .is_some_and(|fact| fact.sufficient(requested))
            {
                continue;
            }
            let pending = work.len();
            work.push(id);
            // Partial input facts may already determine the requested bits.
            // Only visit unresolved inputs when the transfer needs more knowledge.
            let fact = CachedBits::infer(values, facts, id, |input| work.push(input));
            facts.insert(id, fact);
            if fact.sufficient(requested) {
                work.truncate(pending);
            }
        }
        facts[&root].bits
    }
}
