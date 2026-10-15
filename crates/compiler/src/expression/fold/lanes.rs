//! Local lane and byte routing; no graph analysis or demand state is needed.

use super::Folder;
use crate::{
    body::ValueDefinition,
    expression::{BinaryOp as Binary, ShuffleMask, UnaryOp as Unary},
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

    pub(super) fn fold_replace_lane(
        &mut self,
        ty: Type,
        lane_type: Type,
        lane: u8,
        left: usize,
        right: usize,
    ) -> Option<usize> {
        let scalar = self.values.representation(right);
        if let ValueDefinition::Expression(Expression::Unary {
            operator: Unary::ExtractLane { lane: extracted },
            input,
        }) = self.values[scalar].definition
        {
            if self.values[scalar].ty == lane_type
                && lane == extracted
                && self.values.representation(input) == self.values.representation(left)
            {
                return Some(left);
            }
        }
        if let ValueDefinition::Expression(Expression::Binary {
            operator:
                Binary::ReplaceLane {
                    lane_type: previous_type,
                    lane: previous,
                },
            left: base,
            ..
        }) = self.values[self.values.representation(left)].definition
        {
            if lane_type == previous_type && lane == previous {
                return Some(self.fold(
                    ty,
                    Expression::Binary {
                        operator: Binary::ReplaceLane { lane_type, lane },
                        left: base,
                        right,
                    },
                ));
            }
        }
        None
    }

    pub(super) fn shuffle_identity(
        &self,
        indices: &[u8],
        width: u8,
        left: usize,
        right: usize,
    ) -> Option<usize> {
        let same = self.values.representation(left) == self.values.representation(right);
        for (half, input) in [left, right].into_iter().enumerate() {
            if indices.iter().enumerate().all(|(byte, &index)| {
                index < width * 2
                    && usize::from(index % width) == byte
                    && (usize::from(index / width) == half || same)
            }) {
                return Some(input);
            }
        }
        None
    }

    pub(super) fn fold_shuffle(
        &mut self,
        ty: Type,
        mask: ShuffleMask,
        left: usize,
        right: usize,
    ) -> Option<usize> {
        let mut indices = mask.indices();
        if let Some(input) = self.shuffle_identity(&indices, 16, left, right) {
            return Some(input);
        }
        // Compose one level when the resulting routing still needs at most two
        // sources. More general gathers would add operations instead of removing one.
        let mut sources = [None; 2];
        let mut composed = false;
        let inputs = [left, right].map(|input| {
            let input = self.values.representation(input);
            if let ValueDefinition::Expression(Expression::Binary {
                operator: Binary::ShuffleImmediate(inner),
                left,
                right,
            }) = self.values[input].definition
            {
                composed = true;
                (
                    [
                        self.values.representation(left),
                        self.values.representation(right),
                    ],
                    inner.indices(),
                )
            } else {
                ([input; 2], std::array::from_fn(|byte| byte as u8))
            }
        });
        if !composed {
            return None;
        }
        for index in &mut indices {
            let (values, inner) = &inputs[usize::from(*index / 16)];
            let byte = inner[usize::from(*index % 16)];
            let source = values[usize::from(byte / 16)];
            let slot = sources
                .iter()
                .position(|&candidate| candidate == Some(source))
                .or_else(|| sources.iter().position(Option::is_none))?;
            sources[slot] = Some(source);
            *index = slot as u8 * 16 + byte % 16;
        }
        let left = sources[0].unwrap();
        let right = sources[1].unwrap_or(left);
        if let Some(input) = self.shuffle_identity(&indices, 16, left, right) {
            return Some(input);
        }
        Some(self.intern(
            ty,
            Expression::Binary {
                operator: Binary::ShuffleImmediate(ShuffleMask::new(indices)),
                left,
                right,
            },
        ))
    }
}
