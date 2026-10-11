//! Earliest execution sites preserve dominance and authored effect snapshots.

use super::*;

#[derive(Clone, Copy)]
pub(super) struct Requirement {
    pub(super) block: usize,
    after_effect: usize,
}

impl Requirement {
    pub(super) fn available_at(self, site: Self, dominators: &Dominators) -> bool {
        if self.block == site.block {
            self.after_effect <= site.after_effect
        } else {
            dominators.dominates(self.block, site.block)
        }
    }
}

pub(super) fn requirements(
    graph: &FunctionGraph,
    dominators: &Dominators,
) -> Vec<Option<Requirement>> {
    let mut locations = vec![None; graph.effects.len()];
    for (block, data) in graph.blocks.iter().enumerate() {
        for (index, item) in data.items.iter().enumerate() {
            if let BlockItem::Effect(id) = item {
                locations[id.0] = Some(Requirement {
                    block,
                    after_effect: index + 1,
                });
            }
        }
    }
    let mut requirements: Vec<Option<Requirement>> = vec![None; graph.values.len()];
    for (id, value) in graph.values.iter().enumerate() {
        requirements[id] = match value.definition {
            ValueDefinition::Literal(_) => Some(Requirement {
                block: graph.entry.0,
                after_effect: 0,
            }),
            ValueDefinition::Parameter { block, .. } => Some(Requirement {
                block: block.0,
                after_effect: 0,
            }),
            ValueDefinition::Result {
                producer: BlockItem::Effect(effect),
                ..
            } => locations[effect.0],
            ValueDefinition::Result {
                producer: BlockItem::Evaluate(producer),
                ..
            } => requirements[producer],
            ValueDefinition::Expression(expression) => {
                let mut requirement = Some(Requirement {
                    block: graph.entry.0,
                    after_effect: 0,
                });
                for &input in expression.inputs() {
                    requirement = match (requirement, requirements[input]) {
                        (Some(a), Some(b)) if a.block == b.block => Some(Requirement {
                            block: a.block,
                            after_effect: a.after_effect.max(b.after_effect),
                        }),
                        (Some(a), Some(b)) if dominators.dominates(a.block, b.block) => Some(b),
                        (Some(a), Some(b)) if dominators.dominates(b.block, a.block) => Some(a),
                        _ => None,
                    };
                }
                requirement
            }
        };
    }
    requirements
}
