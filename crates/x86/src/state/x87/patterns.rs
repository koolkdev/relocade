//! x87 specialization facts and the retained fields they describe.

use super::X87Tracker;
use crate::{
    ssa::StateFields,
    state::{
        access::cpu_location, ExecutionContext, StatePattern, X87Control, X87State, X87Status,
    },
};
use std::mem::{offset_of, size_of};

#[derive(Clone, Copy)]
pub(crate) enum X87Specialization {
    /// Rounding control alone, for operations independent of precision control.
    Rounding,
    /// Rounding and precision, plus a masked precision flag when already raised.
    Arithmetic,
}

impl X87Specialization {
    pub(crate) fn patterns(self, cpu: &X87State) -> Vec<StatePattern> {
        let mut patterns = vec![rounding(&cpu.control)];
        if matches!(self, Self::Arithmetic) {
            patterns.push(arithmetic_precision(&cpu.control));
            if let Some(precision) = masked_precision(cpu) {
                patterns.extend(precision);
            }
        }
        patterns
    }
}

fn rounding(control: &X87Control) -> StatePattern {
    StatePattern::bits(
        cpu_location!(x87.control.rounding_control),
        0b11,
        u64::from(control.rounding_control),
    )
}

fn arithmetic_precision(control: &X87Control) -> StatePattern {
    StatePattern::bits(
        cpu_location!(x87.control.precision_control),
        0b11,
        u64::from(control.precision_control),
    )
}

fn masked_precision(cpu: &X87State) -> Option<[StatePattern; 2]> {
    let masked = cpu.control.precision_mask & 1 != 0;
    let raised = cpu.status.precision & 1 != 0;
    // Another inexact result cannot change this flag. C1 and numerical rounding
    // still follow their normal rules.
    (masked && raised).then(|| {
        [
            StatePattern::bits(cpu_location!(x87.control.precision_mask), 1, 1),
            StatePattern::bits(cpu_location!(x87.status.precision), 1, 1),
        ]
    })
}

impl X87Tracker {
    pub(in crate::state) fn fields_for(
        &mut self,
        pattern: StatePattern,
    ) -> Option<&mut StateFields> {
        let control = offset_of!(ExecutionContext, cpu.x87.control);
        let status = offset_of!(ExecutionContext, cpu.x87.status);
        if pattern.within_record(control, size_of::<X87Control>()) {
            Some(&mut self.control.fields)
        } else if pattern.within_record(status, size_of::<X87Status>()) {
            Some(&mut self.status.fields)
        } else {
            None
        }
    }
}
