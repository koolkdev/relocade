//! Admit edge facts and merge only executable predecessor definitions.
use super::*;

impl Analysis<'_> {
    pub(super) fn enter(&mut self, block: BlockId, sources: &[usize]) {
        let merges = self.scopes.pending();
        if !sources.is_empty() && sources.iter().all(|&source| self.scopes.complete(source)) {
            for key in merges {
                let root = match key {
                    FactKey::Predicate(id) => {
                        let mut incoming = sources
                            .iter()
                            .map(|&source| self.scopes.predicate_at(source, id));
                        if let Some(Some(truth)) = incoming.next() {
                            if incoming.all(|other| other == Some(truth)) {
                                self.observe_predicate(id, truth, block);
                            }
                        }
                        continue;
                    }
                    FactKey::Value(root) => root,
                };
                let fallback = self.resolve(root);
                if matches!(self.graph.values[root].ty, Type::V64 | Type::V128) {
                    let common = sources
                        .iter()
                        .map(|&source| {
                            let value = self.scopes.value_at(source, root, fallback);
                            self.vector_bits(value)
                        })
                        .reduce(|a, b| [a[0].intersect(b[0]), a[1].intersect(b[1])])
                        .unwrap();
                    self.refine_vector(root, common, block);
                    continue;
                }
                let mut common: Option<Summary> = None;
                for &source in sources {
                    let value = self.scopes.value_at(source, root, fallback);
                    let bits = self.summaries[value];
                    common = Some(common.map_or(bits, |previous| previous.intersect(bits)));
                }
                if let Some(bits) = common {
                    self.assume(root, bits, block);
                }
            }
        }
        // A backedge contributes unknown facts until convergence is established.
        // Acyclic joins can intersect their actual incoming SSA summaries now.
        if sources.iter().all(|&source| self.scopes.complete(source)) {
            for (component, parameter) in self.graph.blocks[block.0]
                .parameters
                .clone()
                .into_iter()
                .enumerate()
            {
                if matches!(self.graph.values[parameter].ty, Type::V64 | Type::V128) {
                    let mut common: Option<[Bits; 2]> = None;
                    for &source in sources {
                        for edge in self.graph.outgoing(BlockId(source)) {
                            if edge.target == block {
                                let bits = self.vector_bits(edge.arguments[component]);
                                common = Some(common.map_or(bits, |previous| {
                                    [
                                        previous[0].intersect(bits[0]),
                                        previous[1].intersect(bits[1]),
                                    ]
                                }));
                            }
                        }
                    }
                    if let Some(mut bits) = common {
                        if self.graph.values[parameter].ty == Type::V64 {
                            bits[1] = Bits::default();
                        }
                        self.lanes.insert(parameter, bits);
                    }
                    continue;
                }
                let mut common: Option<Summary> = None;
                for &source in sources {
                    for edge in self.graph.outgoing(BlockId(source)) {
                        if edge.target == block {
                            let argument = edge.arguments[component];
                            let bits = self.summaries[argument];
                            common = Some(common.map_or(bits, |previous| previous.intersect(bits)));
                        }
                    }
                }
                if let Some(bits) = common {
                    self.summaries[parameter] = self.summaries[parameter].union(bits);
                }
            }
        }
        let [source] = sources else {
            return;
        };
        if !self.scopes.complete(*source) {
            return;
        }
        match self.graph.blocks[*source].exit.clone() {
            Exit::If {
                condition,
                taken,
                otherwise,
            } if taken.target != otherwise.target => {
                self.assume(
                    condition,
                    Bits::constant(u64::from(taken.target == block), 1),
                    block,
                );
            }
            Exit::Switch {
                selector,
                cases,
                default,
            } if default.target != block => {
                let mut matching = cases.into_iter().filter(|(_, edge)| edge.target == block);
                if let (Some((key, _)), None) = (matching.next(), matching.next()) {
                    let mask = self.graph.values[selector].ty.carrier().mask();
                    self.assume(selector, Bits::constant(u64::from(key), mask), block);
                }
            }
            _ => {}
        }
    }
}
