//! Local lane and byte routing; no graph analysis or demand state is needed.

use super::Folder;
use crate::{
    body::ValueDefinition,
    expression::{BinaryOp as Binary, UnaryOp as Unary},
    Expression, Type,
};

impl Folder<'_> {
    pub(super) fn fold_extract_lane(&mut self, ty: Type, lane: u8, input: usize) -> Option<usize> {
        let start = lane * ty.bits();
        let mut base = self.values.representation(input);
        loop {
            match self.values[base].definition {
                ValueDefinition::Expression(Expression::Binary {
                    operator:
                        Binary::ReplaceLane {
                            lane_type,
                            lane: replaced,
                        },
                    left,
                    right,
                }) => {
                    let replaced_start = replaced * lane_type.bits();
                    if ty == lane_type && lane == replaced {
                        // A narrow replacement may have dirty scalar carrier bits.
                        // Extraction zero-extends according to its opcode width.
                        return Some(self.fold_low_bits(ty, right, ty.bits()));
                    }
                    if start < replaced_start + lane_type.bits()
                        && replaced_start < start + ty.bits()
                    {
                        break;
                    }
                    base = self.values.representation(left);
                }
                ValueDefinition::Expression(Expression::Binary {
                    operator: Binary::ShuffleImmediate(mask),
                    left,
                    right,
                }) => {
                    let indices = mask.indices();
                    let width = ty.bits() / 8;
                    let offset = usize::from(lane * width);
                    let first = indices[offset];
                    if first % width != 0
                        || !(0..width).all(|i| indices[offset + usize::from(i)] == first + i)
                    {
                        break;
                    }
                    let source = if first < 16 { left } else { right };
                    return Some(self.fold(
                        ty,
                        Expression::Unary {
                            operator: Unary::ExtractLane {
                                lane: (first % 16) / width,
                            },
                            input: source,
                        },
                    ));
                }
                _ => break,
            }
        }
        (base != self.values.representation(input)).then(|| {
            self.fold(
                ty,
                Expression::Unary {
                    operator: Unary::ExtractLane { lane },
                    input: base,
                },
            )
        })
    }
}
