//! Shared expressions that contain no function-owned values.

use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::{arena::ExpressionArena, expression::Constant, BuildError, Expression, Type};

#[derive(Clone)]
pub(crate) struct UnboundExpression(Rc<Node>);

struct Node {
    ty: Type,
    expression: Expression<Operand>,
}

// Keeping body-owned values out of this type also prevents a reference cycle
// when an arena retains resolved expressions in its admission cache.
#[derive(Clone)]
pub(super) enum Operand {
    Literal(Constant),
    Expression(UnboundExpression),
}

impl UnboundExpression {
    pub(super) fn new(ty: Type, expression: Expression<Operand>) -> Self {
        Self(Rc::new(Node { ty, expression }))
    }

    pub(crate) fn build(&self, arena: &ExpressionArena) -> Result<usize, BuildError> {
        let expression = self.0.expression.try_map(|operand| match operand {
            Operand::Literal(constant) => arena.constant(constant.ty, constant.bits),
            Operand::Expression(expression) => arena.resolve_unbound(expression),
        })?;
        arena.expression(self.0.ty, expression)
    }
}

impl PartialEq for UnboundExpression {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for UnboundExpression {}

impl Hash for UnboundExpression {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}

#[cfg(test)]
mod tests;
