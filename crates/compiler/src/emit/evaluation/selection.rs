//! Select expressions that can share one Wasm instruction or truth operand.
use wasm_encoder::ValType;

use super::{Evaluation, ValuePlanner};
use crate::{body::ValueDefinition, emit::wasm_type, memory::Location, place, Expression, Type};

impl ValuePlanner<'_> {
    pub(super) fn condition_input(&self, condition: usize) -> usize {
        let condition = place::representation(self.body, condition);
        let ValueDefinition::Expression(Expression::ZeroTest {
            input,
            nonzero: true,
        }) = self.body.values[condition].definition
        else {
            return condition;
        };
        // Wasm truth consumers accept any nonzero i32. ZeroTest's input is zero
        // exactly when its logical value is zero; saved Booleans remain zero or one.
        if self.slots[condition].is_none() && wasm_type(self.body.values[input].ty) == ValType::I32
        {
            input
        } else {
            condition
        }
    }

    pub(in crate::emit) fn condition(
        &mut self,
        condition: usize,
        inverted: bool,
    ) -> Vec<Evaluation> {
        let condition = self.condition_input(condition);
        if inverted {
            if let ValueDefinition::Expression(Expression::ZeroTest {
                input,
                nonzero: false,
            }) = self.body.values[condition].definition
            {
                // Inverting an unshared i32 zero-test can use its operand as the
                // Wasm truth value. Saved predicates must keep their original
                // evaluation, and i64 tests must still produce an i32 condition.
                if self.slots[condition].is_none()
                    && wasm_type(self.body.values[input].ty) == ValType::I32
                {
                    return self.values([input]);
                }
            }
        }
        let mut plan = self.values([condition]);
        if inverted {
            plan.push(Evaluation::Expression {
                result_type: Type::I1,
                expression: Expression::ZeroTest {
                    input: Type::I32,
                    nonzero: false,
                },
            });
        }
        plan
    }

    pub(super) fn signed_load_location(&self, mut input: usize) -> Option<Location> {
        let mut bits = self.body.values[input].ty.bits();
        loop {
            input = place::representation(self.body, input);
            // Saved reads and signed values retain their sharing and snapshots.
            if self.slots[input].is_some() {
                return None;
            }
            match self.body.values[input].definition {
                ValueDefinition::Expression(Expression::SignExtend { input: original })
                    if self.body.values[original].ty.bits() <= bits =>
                {
                    // An unshared extension can be covered with the wider one.
                    // Narrowing below its original sign would change the value.
                    bits = self.body.values[original].ty.bits();
                    input = original;
                }
                ValueDefinition::Load { site } => {
                    let location = self.blocks.load_location(site);
                    // Cover the full original read; conversions must not change
                    // its access width or choose a different logical sign bit.
                    return (bits == location.bytes * 8).then_some(location);
                }
                _ => return None,
            }
        }
    }
}
