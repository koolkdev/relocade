//! Refine affected definitions and notify their direct uses.
use super::*;

impl Analysis<'_> {
    pub(super) fn assume(&mut self, root: usize, observed: impl Into<Summary>, block: BlockId) {
        let mut pending = vec![(root, observed.into(), false)];
        let mut budget = 512;
        while let Some((id, mut observed, revisit)) = pending.pop() {
            self.queued[id] = false;
            if budget == 0 {
                break;
            }
            budget -= 1;
            let source = self.runtime_identity(id);
            if revisit {
                observed = observed.union(facts::infer_with(&self.graph.values, source, |input| {
                    self.summaries[self.resolve(input)]
                }));
            }
            let id = self.resolve(id);
            let mask = self.graph.values[id].ty.carrier().mask();
            let observed = observed.restrict(mask);
            if let Some(truth) = observed.known(1) {
                if let Some((index, positive)) = self.predicate_of(id) {
                    self.observe_predicate(index, (truth != 0) == positive, block);
                }
            }
            let previous = self.summaries[id];
            let known = previous.union(observed).restrict(mask);
            if previous.conflicts(observed) || (previous == known && !revisit) {
                continue;
            }
            let mut source = id;
            while let ValueDefinition::RefinedAlias { source: input, .. } =
                self.graph.values[source].definition
            {
                source = input;
            }
            let expression = self.graph.values.expression(source);
            if previous != known {
                let refined = self.refine(id, block);
                self.summaries[refined] = known;
                for user in self.users.of(self.roots[id]) {
                    if !self.graph.values[user].ty.is_integer()
                        || self.queued[user]
                        || !self.definitions[user]
                            .is_some_and(|owner| self.dominators.dominates(owner.0, block.0))
                    {
                        continue;
                    }
                    let current = self.resolve(user);
                    let inferred = facts::infer_with(&self.graph.values, user, |input| {
                        self.summaries[self.resolve(input)]
                    });
                    let summary = self.summaries[current];
                    if summary.union(inferred) != summary || current != user {
                        self.queued[user] = true;
                        pending.push((user, summary, true));
                    }
                }
            }
            let Some(result) = expression else {
                continue;
            };
            if result.component != 0 {
                continue;
            }
            self.observe_lane(id, known.bits, block);
            pending.extend(
                facts::backward_ranges(
                    &self.graph.values,
                    self.graph.values[id].ty,
                    result.expression,
                    known,
                    |input| self.summaries[self.resolve(input)],
                )
                .into_iter()
                .map(|(input, facts)| (input, facts, false)),
            );
            let known = known.bits;
            let mut writes = Vec::new();
            match result.expression {
                Expression::Unary {
                    operator: Unary::Convert,
                    input,
                } => writes.push((input, known)),
                Expression::Unary {
                    operator: Unary::LowBits { bits },
                    input,
                } => writes.push((input, known.restrict(integer::low_mask(bits)))),
                Expression::Unary {
                    operator: Unary::ZeroTest { nonzero },
                    input,
                } => {
                    if let Some(truth) = known.known(1) {
                        if (truth != 0) != nonzero {
                            writes.push((
                                input,
                                Bits::constant(0, self.graph.values[input].ty.carrier().mask()),
                            ));
                        } else if self.graph.values.bounds[input].unsigned <= 1 {
                            writes.push((
                                input,
                                Bits::constant(1, self.graph.values[input].ty.carrier().mask()),
                            ));
                        }
                    }
                }
                Expression::Binary {
                    operator: Binary::Bitwise(operator),
                    left,
                    right,
                } => {
                    let a = self.bits(left);
                    let b = self.bits(right);
                    match operator {
                        BitwiseOp::And => {
                            writes.push((
                                left,
                                Bits {
                                    zero: known.zero & b.one,
                                    one: known.one,
                                },
                            ));
                            writes.push((
                                right,
                                Bits {
                                    zero: known.zero & a.one,
                                    one: known.one,
                                },
                            ));
                        }
                        BitwiseOp::Or => {
                            writes.push((
                                left,
                                Bits {
                                    zero: known.zero,
                                    one: known.one & b.zero,
                                },
                            ));
                            writes.push((
                                right,
                                Bits {
                                    zero: known.zero,
                                    one: known.one & a.zero,
                                },
                            ));
                        }
                        BitwiseOp::Xor => {
                            writes.push((
                                left,
                                Bits {
                                    zero: (known.zero & b.zero) | (known.one & b.one),
                                    one: (known.one & b.zero) | (known.zero & b.one),
                                },
                            ));
                            writes.push((
                                right,
                                Bits {
                                    zero: (known.zero & a.zero) | (known.one & a.one),
                                    one: (known.one & a.zero) | (known.zero & a.one),
                                },
                            ));
                        }
                    }
                }
                Expression::Binary {
                    operator: Binary::Compare(Compare::Integer(operator)),
                    left,
                    right,
                } if matches!(operator, CompareOp::Eq | CompareOp::Ne) => {
                    if known
                        .known(1)
                        .is_some_and(|truth| (truth != 0) == (operator == CompareOp::Eq))
                    {
                        writes.push((left, self.bits(right)));
                        writes.push((right, self.bits(left)));
                    }
                }
                Expression::Ternary {
                    operator: Ternary::Select,
                    first,
                    second,
                    third,
                } => {
                    let condition = self
                        .bits(third)
                        .known(1)
                        .or_else(|| self.bits(first).conflicts(known).then_some(0))
                        .or_else(|| self.bits(second).conflicts(known).then_some(1));
                    if let Some(truth) = condition {
                        writes.push((third, Bits::constant(truth, 1)));
                        writes.push((if truth != 0 { first } else { second }, known));
                    }
                }
                _ => {}
            }
            pending.extend(
                writes
                    .into_iter()
                    .map(|(input, bits)| (input, Summary::from(bits), false)),
            );
        }
        for (id, _, _) in pending {
            self.queued[id] = false;
        }
    }

    pub(super) fn refine(&mut self, id: usize, block: BlockId) -> usize {
        let refined = self.graph.values.push(Value {
            ty: self.graph.values[id].ty,
            definition: ValueDefinition::RefinedAlias { source: id, block },
        });
        self.extend();
        self.definitions[refined] = Some(block);
        let root = self.roots[id];
        self.roots[refined] = root;
        let previous = self.renamed[root].replace(refined);
        self.bindings.push((root << 1, previous));
        self.current = self.versions.set(self.current, block.0, root << 1, refined);
        for &join in &self.frontiers[block.0] {
            if !self.complete[join]
                && self.definitions[root]
                    .is_some_and(|owner| self.dominators.dominates(owner.0, join))
            {
                self.merges[join].push(root << 1);
            }
        }
        refined
    }

    pub(super) fn observe_predicate(&mut self, index: usize, truth: bool, block: BlockId) {
        if self.predicates[index] == Some(truth) {
            return;
        }
        if self.predicates[index].is_some() {
            return;
        }
        let previous = self.predicates[index].replace(truth);
        let key = index << 1 | 1;
        self.bindings.push((key, previous.map(usize::from)));
        self.current = self
            .versions
            .set(self.current, block.0, key, usize::from(truth));
        for &join in &self.frontiers[block.0] {
            if !self.complete[join]
                && self.predicate_keys[index]
                    .inputs()
                    .into_iter()
                    .all(|input| {
                        self.definitions[input]
                            .is_none_or(|owner| self.dominators.dominates(owner.0, join))
                    })
            {
                self.merges[join].push(key);
            }
        }
    }
}
