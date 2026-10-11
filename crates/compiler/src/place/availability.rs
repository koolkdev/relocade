//! Track executions reusable within a dominance scope.
use super::*;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Availability {
    // Value-table IDs are dense and stable throughout placement.
    bindings: Vec<Option<usize>>,
    // A producer can retain its original ID for its first unchanged placement.
    // This history survives scope exit: later executions need distinct IDs.
    placed: Vec<bool>,
    expressions: HashMap<Value, usize>,
    binding_log: Vec<(usize, Option<usize>)>,
    expression_log: Vec<Value>,
}

/// A saved position in this availability owner's active scope.
#[derive(Clone, Copy)]
pub(super) struct Checkpoint {
    bindings: usize,
    expressions: usize,
}

impl Availability {
    pub(super) fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            bindings: self.binding_log.len(),
            expressions: self.expression_log.len(),
        }
    }

    pub(super) fn restore(&mut self, checkpoint: Checkpoint) {
        for (recipe, previous) in self.binding_log.drain(checkpoint.bindings..).rev() {
            self.bindings[recipe] = previous;
        }
        for operation in self.expression_log.drain(checkpoint.expressions..).rev() {
            self.expressions.remove(&operation);
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

    /// Publish an execution binding. Semantic equivalences belong to reduction.
    pub(super) fn bind(&mut self, recipe: usize, value: usize) {
        self.publish(value, value);
        if recipe != value {
            self.publish(recipe, value);
        }
    }

    fn publish(&mut self, recipe: usize, value: usize) {
        if recipe >= self.bindings.len() {
            self.bindings.resize(recipe + 1, None);
        }
        let previous = self.bindings[recipe].replace(value);
        if previous != Some(value) {
            self.binding_log.push((recipe, previous));
        }
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
            let placed = if operation == graph.values[producer]
                && !self.placed.get(producer).copied().unwrap_or(false)
            {
                producer
            } else {
                graph.values.push(operation)
            };
            if placed >= self.placed.len() {
                self.placed.resize(placed + 1, false);
            }
            self.placed[placed] = true;
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
