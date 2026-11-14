//! Rewrite direct SSA uses under dominating refinements.
use super::{
    facts::{self, Bits, Predicate, Summary},
    *,
};
use rustc_hash::FxHashMap;
mod choices;
mod control;
mod lanes;
mod observations;
mod rewrite;
mod scopes;
use scopes::{FactKey, PredicateId, Scopes};
mod uses;
use crate::{
    bitwise::BitwiseOp,
    expression::{
        BinaryOp as Binary, CompareOp as Compare, TernaryOp as Ternary, UnaryOp as Unary,
    },
    integer::{self, CompareOp},
    Type,
};

pub(super) fn run(
    graph: &mut SsaGraph,
    dominators: &Dominators,
    predecessors: &[Vec<usize>],
) -> Vec<bool> {
    let count = graph.values.len();
    let numbers = super::numbering::Numbering::new(&graph.values);
    let mut analysis = Analysis {
        numbers,
        graph,
        dominators,
        scopes: Scopes::new(dominators, predecessors, count),
        aliases: (0..count).collect(),
        summaries: Vec::with_capacity(count),
        executable: vec![false; predecessors.len()],
        emitted: vec![false; count],
        lanes: FxHashMap::default(),
        choices: FxHashMap::default(),
        users: uses::Uses::default(),
        queued: vec![false; count],
    };
    for id in 0..count {
        let bits = facts::infer(&analysis.graph.values, id, &analysis.summaries);
        analysis.summaries.push(bits);
    }
    let mut children = vec![Vec::new(); predecessors.len()];
    for (block, parent) in dominators.parent.iter().enumerate() {
        if let Some(parent) = parent {
            if *parent != block {
                children[*parent].push(block);
            }
        }
    }
    for children in &mut children {
        children.sort_by_key(|&block| dominators.rank[block]);
    }
    let mut work = vec![(analysis.graph.entry.0, None)];
    while let Some((block, checkpoint)) = work.pop() {
        if let Some((facts, uses)) = checkpoint {
            analysis.scopes.leave(facts);
            analysis.users.leave(uses);
            continue;
        }
        let incoming: Vec<_> = predecessors[block]
            .iter()
            .copied()
            .filter(|&source| {
                !analysis.scopes.complete(source)
                    || (analysis.executable[source]
                        && analysis
                            .graph
                            .outgoing(BlockId(source))
                            .iter()
                            .any(|edge| edge.target.0 == block))
            })
            .collect();
        if block != analysis.graph.entry.0
            && !incoming.iter().any(|&source| analysis.executable[source])
        {
            let mut discarded = vec![block];
            while let Some(block) = discarded.pop() {
                analysis.scopes.discard(block);
                discarded.extend(children[block].iter().copied());
            }
            continue;
        }
        analysis.executable[block] = true;
        let checkpoint = (
            analysis.scopes.enter(BlockId(block)),
            analysis.users.checkpoint(),
        );
        analysis.enter(BlockId(block), &incoming);
        analysis.block(BlockId(block));
        analysis.scopes.finish();
        work.push((block, Some(checkpoint)));
        work.extend(children[block].iter().rev().map(|&child| (child, None)));
    }
    analysis.erase()
}

struct Analysis<'a> {
    graph: &'a mut SsaGraph,
    numbers: super::numbering::Numbering,
    dominators: &'a Dominators,
    scopes: Scopes<'a>,
    aliases: Vec<usize>,
    summaries: Vec<Summary>,
    executable: Vec<bool>,
    emitted: Vec<bool>,
    lanes: FxHashMap<usize, [Bits; 2]>,
    choices: FxHashMap<usize, choices::Choice>,
    users: uses::Uses,
    queued: Vec<bool>,
}

impl Analysis<'_> {
    fn extend(&mut self) {
        self.numbers.extend(&self.graph.values);
        self.scopes.extend(self.graph.values.len());
        while self.aliases.len() < self.graph.values.len() {
            let id = self.aliases.len();
            self.aliases.push(id);
            self.emitted.push(false);
            self.queued.push(false);
            self.summaries
                .push(facts::infer(&self.graph.values, id, &self.summaries));
        }
    }

    fn resolve(&self, mut id: usize) -> usize {
        loop {
            if self.aliases[id] != id {
                id = self.aliases[id];
            } else if let Some(next) = self.scopes.refinement(id).filter(|&next| next != id) {
                id = next;
            } else {
                return id;
            }
        }
    }

    fn runtime_identity(&self, mut id: usize) -> usize {
        loop {
            if self.aliases[id] != id {
                id = self.aliases[id];
            } else if let ValueDefinition::RefinedAlias { source: input, .. } =
                self.graph.values[id].definition
            {
                id = input;
            } else {
                return id;
            }
        }
    }

    fn predicate_for(&mut self, expression: Expression<usize>) -> Option<(PredicateId, bool)> {
        let (key, positive) = facts::predicate(&self.graph.values, expression, |input| {
            self.numbers.get(
                self.graph
                    .values
                    .representation(self.runtime_identity(input)),
            )
        })?;
        Some((self.scopes.predicate_id(key), positive))
    }

    fn predicate_of(&mut self, id: usize) -> Option<(PredicateId, bool)> {
        let source = self.runtime_identity(id);
        if let Some(expression) = self.graph.values.expression(source) {
            if expression.component == 0 {
                if let Some(predicate) = self.predicate_for(expression.expression) {
                    return Some(predicate);
                }
            }
        }
        (self.graph.values[source].ty == Type::I1 && self.graph.values.bounds[source].unsigned <= 1)
            .then(|| {
                (
                    self.scopes
                        .predicate_id(Predicate::Zero(self.numbers.get(source))),
                    false,
                )
            })
    }

    fn bits(&self, id: usize) -> Bits {
        self.summaries[self.resolve(id)].bits
    }

    fn operand(&mut self, id: usize) -> usize {
        let mut id = self.resolve(id);
        let source = self.runtime_identity(id);
        if self.graph.values[id].ty == Type::I1
            && self
                .graph
                .values
                .expression(source)
                .is_some_and(|result| self.emitted[result.producer])
        {
            if let Some((index, positive)) = self.predicate_of(id) {
                if let Some(truth) = self.scopes.predicate(index) {
                    self.assume(
                        id,
                        Bits::constant(u64::from(truth == positive), 1),
                        self.scopes.block(),
                    );
                    id = self.resolve(id);
                }
            }
        }
        let value = self.graph.values[id];
        if matches!(
            value.ty,
            Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64
        ) {
            if let Some(bits) = self.summaries[id].known(value.ty.carrier().mask()) {
                let literal = self.graph.values.carrier_literal(value.ty, bits);
                self.extend();
                return literal;
            }
        }
        id
    }
}
