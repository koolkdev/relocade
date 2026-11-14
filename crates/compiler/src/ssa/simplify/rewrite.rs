//! Rewrite direct uses, materialize surviving definitions, and erase proof aliases.
use super::*;

impl Analysis<'_> {
    pub(super) fn block(&mut self, block: BlockId) {
        let items = std::mem::take(&mut self.graph.blocks[block.0].items);
        for item in items {
            match item {
                BlockItem::Evaluate(id) => self.calculation(id, block),
                BlockItem::Effect(id) => {
                    let operation = self.graph.effects[id.0]
                        .operation
                        .clone()
                        .map_inputs(|input| self.operand(input));
                    self.graph.effects[id.0].operation = operation;
                    self.graph.blocks[block.0].items.push(item);
                }
            }
        }
        let mut exit = self.graph.blocks[block.0].exit.clone();
        exit.map_inputs(|input| self.operand(input));
        self.graph.blocks[block.0].exit = exit;
    }

    pub(super) fn calculation(&mut self, id: usize, block: BlockId) {
        let source = self.graph.values.expression(id).unwrap();
        let expression = source.expression.map(|&input| self.operand(input));
        if let Some((index, positive)) = self.predicate_for(expression) {
            if let Some(truth) = self.predicates[index] {
                let constant = self
                    .graph
                    .values
                    .literal(Type::I1, u64::from(truth == positive));
                self.extend();
                self.aliases[id] = constant;
                return;
            }
        }
        let outputs: Vec<_> = self.graph.values.expression_results(id).collect();
        for (component, output) in outputs.into_iter().enumerate() {
            let ty = self.graph.values[output].ty;
            let result = if expression == source.expression {
                output
            } else {
                crate::expression::fold_carrier(&mut self.graph.values, ty, expression, component)
            };
            self.extend();
            let result = self.identity(result).unwrap_or(result);
            // Folding can return an ancestor's existing definition. Facts
            // learned here belong to a refinement, never to that definition.
            let fresh = self
                .graph
                .values
                .expression(result)
                .is_some_and(|result| !self.emitted[result.producer])
                && self.definitions[result].is_none_or(|owner| owner == block);
            if fresh {
                self.summaries[result] = self.summaries[result].union(self.infer(result));
                self.infer_choice(result);
                if matches!(self.graph.values[result].ty, Type::V64 | Type::V128) {
                    let bits = self.vector_bits(result);
                    if bits != [Bits::default(); 2] {
                        self.lanes.insert(result, bits);
                    }
                }
            }
            let result = self.operand(result);
            self.emit(result, block);
            self.aliases[output] = result;
        }
    }

    pub(super) fn identity(&self, id: usize) -> Option<usize> {
        if !matches!(
            self.graph.values[id].ty,
            Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64
        ) {
            return None;
        }
        let result = self.graph.values.expression(id)?;
        if result.component != 0 {
            return None;
        }
        let Expression::Binary {
            operator: Binary::Bitwise(operator),
            left,
            right,
        } = result.expression
        else {
            return None;
        };
        let mask = self.graph.values[id].ty.carrier().mask();
        let a = self.bits(left);
        let b = self.bits(right);
        for (input, own, other) in [(left, a, b), (right, b, a)] {
            let identity = match operator {
                BitwiseOp::And => (own.zero | other.one) & mask == mask,
                BitwiseOp::Or => (own.one | other.zero) & mask == mask,
                BitwiseOp::Xor => other.zero & mask == mask,
            };
            if identity {
                return Some(input);
            }
        }
        None
    }

    pub(super) fn emit(&mut self, root: usize, block: BlockId) {
        let mut work = vec![(root, false)];
        while let Some((id, ready)) = work.pop() {
            let Some(result) = self.graph.values.expression(id) else {
                continue;
            };
            if self.emitted[result.producer] {
                continue;
            }
            if let Some(owner) = self.definitions[id] {
                if owner != block {
                    debug_assert!(self.dominators.dominates(owner.0, block.0));
                    continue;
                }
            }
            if ready {
                self.emitted[result.producer] = true;
                let expression = result.expression.map(|&input| self.runtime_identity(input));
                self.numbers
                    .define(&self.graph.values, result.producer, expression);
                for output in self.graph.values.expression_results(result.producer) {
                    for &input in result.expression.inputs() {
                        self.users.add(self.roots[input], output);
                    }
                }
                self.graph.blocks[block.0]
                    .items
                    .push(BlockItem::Evaluate(result.producer));
                for output in self.graph.values.expression_results(result.producer) {
                    self.definitions[output] = Some(block);
                }
            } else {
                work.push((id, true));
                work.extend(
                    result
                        .expression
                        .inputs()
                        .rev()
                        .map(|&input| (input, false)),
                );
            }
        }
    }

    pub(super) fn erase(&mut self) -> Vec<bool> {
        // All refinements are proof-only aliases. Their clients have already
        // been simplified, so the backend only needs ordinary runtime values.
        let mut runtime = Vec::with_capacity(self.graph.values.len());
        for mut id in 0..self.graph.values.len() {
            loop {
                if self.aliases[id] != id {
                    id = self.aliases[id];
                } else if let ValueDefinition::RefinedAlias { source: input, .. } =
                    self.graph.values[id].definition
                {
                    id = input;
                } else {
                    break;
                }
            }
            runtime.push(id);
        }
        let mut changed = vec![false; self.graph.values.len()];
        for (id, value) in self.graph.values.values.iter_mut().enumerate() {
            if let ValueDefinition::Expression(expression) = value.definition {
                let erased = expression.map(|&input| runtime[input]);
                changed[id] = erased != expression;
                value.definition = ValueDefinition::Expression(erased);
            }
        }
        for effect in &mut self.graph.effects {
            effect.operation = effect.operation.clone().map_inputs(|input| runtime[input]);
        }
        for block in &mut self.graph.blocks {
            block.exit.map_inputs(|input| runtime[input]);
        }
        changed
    }
}
