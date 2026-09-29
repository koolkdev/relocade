//! Specialize recipes before scheduling their remaining dependencies.
use super::*;

impl Placer<'_> {
    pub(super) fn materialize(&mut self, root: usize, block: BlockId) -> usize {
        enum Work {
            Visit(usize),
            Finish(usize),
            Select(usize, usize, usize, usize),
            Alias(usize, usize),
        }
        let mut work = vec![Work::Visit(root)];
        while let Some(task) = work.pop() {
            match task {
                Work::Visit(id) => {
                    if self.specialized.contains_key(&id) {
                        continue;
                    }
                    let value = self.graph.values[id];
                    if let Some(bits) = self.facts.constant(&self.graph.values, id) {
                        let bits = self.graph.values.carrier_bits(id, bits);
                        let result = self.graph.values.intern(Value {
                            ty: value.ty,
                            definition: ValueDefinition::Constant(bits),
                        });
                        self.specialized.insert(id, result);
                    } else if let Some(result) = self.available.get(&id).copied() {
                        self.specialized.insert(id, result);
                    } else if let ValueDefinition::Expression(expression) = value.definition {
                        if let Some(result) = self.joins.available_at(self.graph, block.0, id) {
                            self.define(id, result);
                            self.specialized.insert(id, result);
                            continue;
                        }
                        if let Expression::Select {
                            condition,
                            when_true,
                            when_false,
                        } = expression
                        {
                            work.push(Work::Select(id, condition, when_true, when_false));
                            work.push(Work::Visit(condition));
                        } else {
                            work.push(Work::Finish(id));
                            work.extend(expression.inputs().rev().map(|&v| Work::Visit(v)));
                        }
                    } else {
                        self.specialized.insert(id, id);
                    }
                }
                Work::Select(id, condition, when_true, when_false) => {
                    let condition = self.specialized[&condition];
                    if let ValueDefinition::Constant(bits) = self.graph.values[condition].definition
                    {
                        let input = if bits == 0 { when_false } else { when_true };
                        work.push(Work::Alias(id, input));
                        work.push(Work::Visit(input));
                    } else {
                        work.push(Work::Finish(id));
                        work.push(Work::Visit(when_false));
                        work.push(Work::Visit(when_true));
                    }
                }
                Work::Alias(id, input) => {
                    self.specialized.insert(id, self.specialized[&input]);
                }
                Work::Finish(id) => {
                    let result = crate::expression::map_inputs(&mut self.graph.values, id, |v| {
                        self.specialized[v]
                    });
                    self.specialized.insert(id, result);
                }
            }
        }
        let residual = self.specialized[&root];
        let mut work = vec![(residual, false)];
        while let Some((id, ready)) = work.pop() {
            if self.available.contains_key(&id) {
                continue;
            }
            let value = self.graph.values[id];
            let ValueDefinition::Expression(expression) = value.definition else {
                if let ValueDefinition::Result { effect, .. } = value.definition {
                    panic!(
                        "effect result {id} from {} unavailable in block {}",
                        effect.0, block.0
                    );
                }
                continue;
            };
            if !ready {
                work.push((id, true));
                work.extend(expression.inputs().rev().map(|&v| (v, false)));
                continue;
            }
            let expression = expression.map(|v| self.available.get(v).copied().unwrap_or(*v));
            let key = Value {
                ty: value.ty,
                definition: ValueDefinition::Expression(expression),
            };
            let placed = if let Some(&placed) = self.expressions.get(&key) {
                placed
            } else {
                let placed = self.graph.values.push(key);
                self.graph.blocks[block.0]
                    .items
                    .push(BlockItem::Evaluate(placed));
                self.expression_log
                    .push((key, self.expressions.insert(key, placed)));
                self.define(placed, placed);
                placed
            };
            self.define(id, placed);
        }
        let result = self.available.get(&residual).copied().unwrap_or(residual);
        self.define(root, result);
        result
    }

    pub(super) fn define(&mut self, id: usize, result: usize) {
        self.available_log
            .push((id, self.available.insert(id, result)));
    }
}
