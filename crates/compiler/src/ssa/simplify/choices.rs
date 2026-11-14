//! Preserve one selector's correlation in direct scalar transfers.
//! Each alternative interprets this operation only, using summaries of its inputs.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Choice {
    selector: usize,
    taken: Summary,
    otherwise: Summary,
}

impl Analysis<'_> {
    fn choice_input(&self, input: usize) -> usize {
        self.numbers.get(
            self.graph
                .values
                .representation(self.runtime_identity(input)),
        )
    }

    pub(super) fn infer_choice(&mut self, id: usize) {
        if !self.graph.values[id].ty.is_integer() {
            return;
        }
        let Some(result) = self.graph.values.expression(id) else {
            return;
        };
        if result.component != 0 {
            return;
        }
        let selector = match result.expression {
            Expression::Ternary {
                operator: Ternary::Select,
                third,
                ..
            } => Some(self.choice_input(third)),
            Expression::Unary {
                operator: Unary::Convert | Unary::LowBits { .. } | Unary::ZeroTest { .. },
                ..
            }
            | Expression::Binary {
                operator: Binary::Bitwise(_) | Binary::Compare(_),
                ..
            }
            | Expression::Shift { .. } => result.expression.inputs().find_map(|&input| {
                self.choices
                    .get(&self.runtime_identity(input))
                    .map(|choice| choice.selector)
            }),
            _ => None,
        };
        let Some(selector) = selector else {
            return;
        };
        let infer = |taken| {
            facts::infer_with(&self.graph.values, id, |input| {
                let base = self.summaries[self.resolve(input)];
                if self.graph.values[input].ty == Type::I1 && self.choice_input(input) == selector {
                    return base.union(
                        Bits::constant(
                            u64::from(taken),
                            self.graph.values[input].ty.carrier().mask(),
                        )
                        .into(),
                    );
                }
                if let Some(choice) = self
                    .choices
                    .get(&self.runtime_identity(input))
                    .filter(|choice| choice.selector == selector)
                {
                    let alternative = if taken {
                        choice.taken
                    } else {
                        choice.otherwise
                    };
                    if !base.conflicts(alternative) {
                        return base.union(alternative);
                    }
                }
                base
            })
        };
        let taken = infer(true);
        let otherwise = infer(false);
        let base = self.summaries[id];
        if base.conflicts(taken) || base.conflicts(otherwise) {
            return;
        }
        self.summaries[id] = base.union(taken.intersect(otherwise));
        if taken != otherwise {
            self.choices.insert(
                id,
                Choice {
                    selector,
                    taken,
                    otherwise,
                },
            );
        }
    }
}
