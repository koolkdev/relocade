//! Shared expressions that contain no function-owned values.

use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::{arena::FunctionArena, expression::TypedLiteral, BuildError, Expression, Type};

#[derive(Clone)]
pub(crate) struct UnboundExpression {
    node: Rc<Node>,
    ty: Type,
    component: u8,
}

struct Node {
    expression: Expression<Operand>,
}

// Keeping body-owned values out of this type also prevents a reference cycle
// when an arena retains resolved expressions in its admission cache.
#[derive(Clone)]
pub(super) enum Operand {
    Literal(TypedLiteral),
    Expression(UnboundExpression),
}

impl UnboundExpression {
    pub(super) fn new(ty: Type, expression: Expression<Operand>) -> Self {
        Self {
            node: Rc::new(Node { expression }),
            ty,
            component: 0,
        }
    }

    pub(super) fn result(&self, component: usize) -> Self {
        debug_assert!(component < self.node.expression.result_types(self.ty).len());
        Self {
            node: self.node.clone(),
            ty: self.ty,
            component: component
                .try_into()
                .expect("expression result index fits in a byte"),
        }
    }

    pub(crate) fn build(&self, arena: &FunctionArena) -> Result<usize, BuildError> {
        let expression = self.node.expression.try_map(|operand| match operand {
            Operand::Literal(literal) => arena.literal(literal.ty, literal.value),
            Operand::Expression(expression) => arena.resolve_unbound(expression),
        })?;
        arena.expression(self.ty, expression, usize::from(self.component))
    }
}

impl PartialEq for UnboundExpression {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.node, &other.node) && self.component == other.component
    }
}

impl Eq for UnboundExpression {}

impl Hash for UnboundExpression {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.node).hash(state);
        self.component.hash(state);
    }
}

#[cfg(test)]
mod tests;
