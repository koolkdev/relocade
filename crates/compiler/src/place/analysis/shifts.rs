//! Shift known physical bits using Wasm's carrier width and masked counts.

use super::Bits;
use crate::{
    body::ValueTable,
    integer::{low_mask, ShiftOp},
};

#[cfg(test)]
mod tests;

impl Bits {
    pub(super) fn shifted(
        self,
        table: &ValueTable,
        input: usize,
        operator: ShiftOp,
        count: Self,
    ) -> Self {
        let width = table[input].ty.carrier().bits();
        let count_mask = u64::from(width - 1);
        if count.mask & count_mask != count_mask {
            return Self::default();
        }
        let count = (count.value & count_mask) as u32;
        let carrier_mask = low_mask(width);
        // A narrow logical view can still carry upper physical bits. Only
        // literals and physical bounds establish facts about those bits.
        let bits = match table[input].scalar_literal() {
            Some(value) => Self {
                mask: carrier_mask,
                value,
            },
            None => self.union(Self {
                mask: carrier_mask & !low_mask(table.bounds[input].unsigned),
                value: 0,
            }),
        };
        match operator {
            ShiftOp::Left => Self {
                mask: (bits.mask << count) | low_mask(count as u8),
                value: bits.value << count,
            },
            ShiftOp::RightUnsigned => Self {
                mask: (bits.mask >> count) | (carrier_mask & !low_mask(width - count as u8)),
                value: bits.value >> count,
            },
            ShiftOp::RightSigned => Self::default(),
        }
        .restrict(carrier_mask)
    }
}
