//! Unsigned intervals for comparisons on normalized logical values.

use super::{Bits, ValueAnalysis};
use crate::{body::ValueTable, integer::CompareOp};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Range {
    pub(super) minimum: u64,
    pub(super) maximum: u64,
}

impl Range {
    fn intersect(self, other: Self) -> Option<Self> {
        let minimum = self.minimum.max(other.minimum);
        let maximum = self.maximum.min(other.maximum);
        (minimum <= maximum).then_some(Self { minimum, maximum })
    }

    pub(super) fn compare(self, operator: CompareOp, other: Self) -> Option<bool> {
        match operator {
            CompareOp::Eq | CompareOp::Ne => {
                let equal = if self.maximum < other.minimum || other.maximum < self.minimum {
                    false
                } else if self.minimum == self.maximum && other.minimum == other.maximum {
                    true
                } else {
                    return None;
                };
                Some(equal == (operator == CompareOp::Eq))
            }
            CompareOp::LtUnsigned | CompareOp::GeUnsigned => {
                let less = if self.maximum < other.minimum {
                    true
                } else if self.minimum >= other.maximum {
                    false
                } else {
                    return None;
                };
                Some(less == (operator == CompareOp::LtUnsigned))
            }
            _ => None,
        }
    }
}

impl ValueAnalysis {
    pub(super) fn range(&self, table: &ValueTable, id: usize, bits: Bits) -> Option<Range> {
        // Unsigned comparisons observe carriers. Logical intervals describe
        // those carriers only after their unused upper bits have been cleared.
        if table.bounds[id].unsigned > table[id].ty.bits() {
            return None;
        }
        let logical = Range {
            minimum: bits.value,
            maximum: bits.value | (table[id].ty.mask() & !bits.mask),
        };
        Some(
            self.context
                .ranges
                .get(&id)
                .and_then(|&range| logical.intersect(range))
                .unwrap_or(logical),
        )
    }

    fn restrict_range(&mut self, id: usize, range: Range) {
        if range.minimum > range.maximum {
            return;
        }
        let Some(range) = self
            .context
            .ranges
            .get(&id)
            .map_or(Some(range), |previous| previous.intersect(range))
        else {
            return;
        };
        if self.context.ranges.insert(id, range) != Some(range) {
            self.invalidate_from(id);
        }
    }

    pub(super) fn assume_nonzero(&mut self, table: &ValueTable, input: usize) {
        // Ordered comparisons with zero canonicalize to a zero test.
        if let Some(range) = self.range(table, input, self.bits(table, input)) {
            self.restrict_range(
                input,
                Range {
                    minimum: 1,
                    maximum: range.maximum,
                },
            );
        }
    }

    pub(super) fn assume_comparison(
        &mut self,
        table: &ValueTable,
        operator: CompareOp,
        left: usize,
        right: usize,
        truth: bool,
    ) {
        if !matches!(operator, CompareOp::LtUnsigned | CompareOp::GeUnsigned) {
            return;
        }
        let (Some(a), Some(b)) = (
            self.range(table, left, self.bits(table, left)),
            self.range(table, right, self.bits(table, right)),
        ) else {
            return;
        };
        if truth != (operator == CompareOp::LtUnsigned) {
            // The negated strict comparison includes equality.
            self.restrict_range(
                left,
                Range {
                    minimum: b.minimum,
                    maximum: table[left].ty.mask(),
                },
            );
            self.restrict_range(
                right,
                Range {
                    minimum: 0,
                    maximum: a.maximum,
                },
            );
            return;
        }
        if let Some(maximum) = b.maximum.checked_sub(1) {
            self.restrict_range(
                left,
                Range {
                    minimum: 0,
                    maximum,
                },
            );
        }
        if let Some(minimum) = a.minimum.checked_add(1) {
            self.restrict_range(
                right,
                Range {
                    minimum,
                    maximum: table[right].ty.mask(),
                },
            );
        }
    }
}
