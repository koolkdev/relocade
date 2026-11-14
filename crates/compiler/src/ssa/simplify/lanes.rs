//! Scalar lane observations refine the vector definition they read.
use super::*;

impl Analysis<'_> {
    pub(super) fn vector_bits(&self, id: usize) -> [Bits; 2] {
        if let Some(&bits) = self.lanes.get(&id) {
            return bits;
        }
        match self.graph.values[id].definition {
            ValueDefinition::Literal(literal) => {
                let bits = u128::from(literal);
                [
                    Bits::constant(bits as u64, u64::MAX),
                    Bits::constant((bits >> 64) as u64, u64::MAX),
                ]
            }
            ValueDefinition::Refine { input, .. } => self.vector_bits(input),
            ValueDefinition::Expression(Expression::Unary {
                operator: Unary::Reinterpret | Unary::Convert,
                input,
            }) => {
                if matches!(self.graph.values[input].ty, Type::V64 | Type::V128) {
                    self.vector_bits(input)
                } else if self.graph.values[input].ty == Type::I64 {
                    [self.summaries[input].bits, Bits::default()]
                } else {
                    [Bits::default(); 2]
                }
            }
            _ => [Bits::default(); 2],
        }
    }

    pub(super) fn infer(&self, id: usize) -> Summary {
        let base = facts::infer(&self.graph.values, id, &self.summaries);
        if let Some((input, half, offset, mask)) = self.lane(id) {
            let bits = self.vector_bits(self.resolve(input))[half];
            return base.union(
                Bits {
                    zero: bits.zero >> offset,
                    one: bits.one >> offset,
                }
                .restrict(mask)
                .into(),
            );
        }
        base
    }

    fn lane(&self, id: usize) -> Option<(usize, usize, u8, u64)> {
        let value = self.graph.values[id];
        let ValueDefinition::Expression(Expression::Unary { operator, input }) = value.definition
        else {
            return None;
        };
        let lane = match operator {
            Unary::ExtractLane { lane } => lane,
            Unary::Reinterpret
                if value.ty == Type::I64
                    && matches!(self.graph.values[input].ty, Type::V64 | Type::V128) =>
            {
                0
            }
            _ => return None,
        };
        let start = lane * value.ty.bits();
        Some((input, usize::from(start / 64), start % 64, value.ty.mask()))
    }

    pub(super) fn observe_lane(&mut self, id: usize, known: Bits, block: BlockId) {
        let source = self.runtime_identity(id);
        let Some((input, half, offset, mask)) = self.lane(source) else {
            return;
        };
        let mut bits = [Bits::default(); 2];
        let known = known.restrict(mask);
        bits[half] = Bits {
            zero: known.zero << offset,
            one: known.one << offset,
        };
        self.refine_vector(input, bits, block);
    }

    pub(super) fn refine_vector(&mut self, id: usize, observed: [Bits; 2], block: BlockId) {
        let id = self.resolve(id);
        let previous = self.vector_bits(id);
        if (0..2).any(|half| previous[half].conflicts(observed[half])) {
            return;
        }
        let known = [
            previous[0].union(observed[0]),
            previous[1].union(observed[1]),
        ];
        if known == previous {
            return;
        }
        let refined = self.refine(id, block);
        self.lanes.insert(refined, known);
    }
}
