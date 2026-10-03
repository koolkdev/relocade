//! Value selection keeps its condition's truth distinct from its numeric bits.

use super::Folder;
use crate::{body::ValueDefinition, Expression, Type};

impl Folder<'_> {
    pub(super) fn select(
        &mut self,
        condition: usize,
        when_true: usize,
        when_false: usize,
    ) -> usize {
        let condition = self.normalize(condition);
        self.fold(
            self.values[when_true].ty,
            Expression::Select {
                condition,
                when_true,
                when_false,
            },
        )
    }

    pub(super) fn fold_select(
        &mut self,
        condition: usize,
        when_true: usize,
        when_false: usize,
    ) -> Option<usize> {
        match self.values[condition].definition {
            ValueDefinition::Constant(0) => return Some(when_false),
            ValueDefinition::Constant(_) => return Some(when_true),
            _ => {}
        }
        if self.values.representation(when_true) == self.values.representation(when_false) {
            return Some(when_true);
        }
        if !self.values[when_true].ty.is_integer() {
            return None;
        }
        let nonzero = match (
            self.values[when_true].definition,
            self.values[when_false].definition,
        ) {
            (ValueDefinition::Constant(1), ValueDefinition::Constant(0)) => true,
            (ValueDefinition::Constant(0), ValueDefinition::Constant(1)) => false,
            _ => return None,
        };
        // A truth consumer accepts any nonzero carrier. Numeric selection must
        // still produce exactly zero or one, including during operand refolding.
        Some(self.fold(
            Type::I1,
            Expression::ZeroTest {
                input: condition,
                nonzero,
            },
        ))
    }
}
