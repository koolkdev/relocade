//! Track reusable values and their recipe aliases within a dominance scope.
use super::*;
use specialize::Alias;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Availability {
    // Value-table IDs are dense and stable throughout placement.
    bindings: Vec<Option<usize>>,
    expressions: HashMap<Value, usize>,
    // A residual may execute after specialization reports its original recipes.
    // Keep these links until scope exit, including after publishing a binding.
    aliases: Vec<Vec<usize>>,
    binding_log: Vec<(usize, Option<usize>)>,
    expression_log: Vec<Value>,
    alias_log: Vec<usize>,
}

/// A saved position in this availability owner's active scope.
#[derive(Clone, Copy)]
pub(super) struct Checkpoint {
    bindings: usize,
    expressions: usize,
    aliases: usize,
}

pub(super) struct RecipeBits {
    pub(super) recipe: usize,
    pub(super) mask: u64,
}

impl Availability {
    pub(super) fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            bindings: self.binding_log.len(),
            expressions: self.expression_log.len(),
            aliases: self.alias_log.len(),
        }
    }

    pub(super) fn restore(&mut self, checkpoint: Checkpoint) {
        for (recipe, previous) in self.binding_log.drain(checkpoint.bindings..).rev() {
            self.bindings[recipe] = previous;
        }
        for operation in self.expression_log.drain(checkpoint.expressions..).rev() {
            self.expressions.remove(&operation);
        }
        for residual in self.alias_log.drain(checkpoint.aliases..).rev() {
            self.aliases[residual].pop();
        }
    }

    pub(super) fn bindings_since(
        &self,
        checkpoint: Checkpoint,
    ) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.binding_log[checkpoint.bindings..]
            .iter()
            .filter_map(|&(recipe, previous)| {
                let value = self.bindings[recipe].unwrap();
                (previous != Some(value)).then_some((recipe, value))
            })
    }

    pub(super) fn get(&self, recipe: usize) -> Option<usize> {
        self.bindings.get(recipe).copied().flatten()
    }

    /// Query reusable values without adding calculations or join parameters.
    pub(super) fn lookup(&self, values: &ValueTable, recipe: usize) -> Option<usize> {
        if let Some(value) = self.get(recipe) {
            return Some(value);
        }
        let result = values.expression(recipe)?;
        let &producer = self.expressions.get(&values[result.producer])?;
        Some(values.expression_result(producer, result.component))
    }

    pub(super) fn record_aliases(&mut self, aliases: impl IntoIterator<Item = Alias>) {
        for Alias { recipe, residual } in aliases {
            if recipe == residual || self.get(recipe).is_some() {
                continue;
            }
            self.link(recipe, residual);
            if let Some(value) = self.get(residual) {
                self.publish(recipe, value);
            }
        }
    }

    /// Bind a recipe to a value already usable in this scope.
    pub(super) fn bind(&mut self, recipe: usize, value: usize) {
        // A join may supply an execution from a scope already left behind.
        // Its value is reusable too, independently of this recipe's alias.
        if self.get(value) != Some(value) {
            self.publish(value, value);
        }
        // Publishing the value may already bind the recipe through aliases.
        // Keep that path's width limits instead of adding a shortcut.
        if recipe != value && self.get(recipe) != Some(value) {
            self.link(recipe, value);
            self.publish(recipe, value);
        }
    }

    fn publish(&mut self, recipe: usize, value: usize) {
        let mut pending = vec![recipe];
        while let Some(recipe) = pending.pop() {
            if recipe >= self.bindings.len() {
                self.bindings.resize(recipe + 1, None);
            }
            let previous = self.bindings[recipe].replace(value);
            if previous == Some(value) {
                continue;
            }
            self.binding_log.push((recipe, previous));
            // Propagated bindings keep their original alias path. Flattening
            // that path here would lose any intervening logical-width limit.
            if let Some(aliases) = self.aliases.get(recipe) {
                pending.extend(
                    aliases
                        .iter()
                        .copied()
                        .filter(|alias| self.get(*alias).is_none()),
                );
            }
        }
    }

    fn link(&mut self, recipe: usize, residual: usize) {
        if residual >= self.aliases.len() {
            self.aliases.resize_with(residual + 1, Vec::new);
        }
        self.aliases[residual].push(recipe);
        self.alias_log.push(residual);
    }

    /// Recover source recipes for an observed result. An alias preserves its
    /// logical bits; it does not promise equality of unknown upper carrier bits.
    pub(super) fn sources(&self, values: &ValueTable, value: usize, mask: u64) -> Vec<RecipeBits> {
        let mut pending = vec![RecipeBits {
            recipe: value,
            mask,
        }];
        let mut seen = HashMap::<usize, u64>::new();
        let mut sources = Vec::new();
        while let Some(RecipeBits { recipe, mask }) = pending.pop() {
            let mask = mask & values[recipe].ty.mask();
            let known = seen.entry(recipe).or_default();
            let mask = mask & !*known;
            if mask == 0 {
                continue;
            }
            *known |= mask;
            sources.push(RecipeBits { recipe, mask });
            if let Some(aliases) = self.aliases.get(recipe) {
                pending.extend(aliases.iter().map(|&recipe| RecipeBits { recipe, mask }));
            }
        }
        sources
    }

    /// Place one producer and make all of its results available together.
    pub(super) fn evaluate(&mut self, graph: &mut FunctionGraph, block: BlockId, producer: usize) {
        let result = graph.values.expression(producer).unwrap();
        let expression = result
            .expression
            .map(|&input| self.get(input).unwrap_or(input));
        let operation = Value {
            ty: graph.values[producer].ty,
            definition: ValueDefinition::Expression(expression),
        };
        let placed = if let Some(&placed) = self.expressions.get(&operation) {
            placed
        } else {
            let placed = graph.values.push(operation);
            graph.blocks[block.0]
                .items
                .push(BlockItem::Evaluate(placed));
            self.expressions.insert(operation, placed);
            self.expression_log.push(operation);
            placed
        };
        for (recipe, output) in graph
            .values
            .expression_results(producer)
            .zip(graph.values.expression_results(placed))
        {
            self.bind(recipe, output);
        }
    }
}
