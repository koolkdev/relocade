//! Shared construction handles, lexical scopes and unbound-expression admission.
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::{
    body::{Site, Value, ValueDefinition, ValueTable},
    value::UnboundExpression,
    BuildError, Expression, Type,
};

#[derive(Clone)]
pub(super) struct ExpressionArena(Rc<RefCell<Option<Construction>>>);

#[derive(Default)]
struct Construction {
    table: ValueTable,
    unbound: HashMap<UnboundExpression, usize>,
    scopes: Vec<usize>,
}

impl ExpressionArena {
    pub(super) fn new() -> Self {
        Self(Rc::new(RefCell::new(Some(Construction {
            scopes: vec![0],
            ..Construction::default()
        }))))
    }

    pub(super) fn same_body(&self, other: &Self) -> bool {
        // Handles keep this allocation alive, even after the builder closes it.
        // A new body therefore cannot acquire an old handle's identity.
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub(super) fn check_open(&self) -> Result<(), BuildError> {
        if self.0.borrow().is_some() {
            Ok(())
        } else {
            Err(BuildError::BodyClosed)
        }
    }

    pub(super) fn intern(&self, value: Value) -> Result<usize, BuildError> {
        self.with_open(|table| table.intern(value))
    }

    pub(super) fn constant(&self, ty: Type, bits: u64) -> Result<usize, BuildError> {
        self.with_open(|table| table.constant(ty, bits))
    }

    pub(crate) fn resolve_unbound(
        &self,
        expression: &UnboundExpression,
    ) -> Result<usize, BuildError> {
        {
            let arena = self.0.borrow();
            let arena = arena.as_ref().ok_or(BuildError::BodyClosed)?;
            if let Some(&value) = arena.unbound.get(expression) {
                return Ok(value);
            }
        }
        // Recursive admission may need this arena again. Build outside the borrow,
        // then retain the recipe identity so a shared DAG is traversed only once.
        let value = expression.build(self)?;
        let mut arena = self.0.borrow_mut();
        let arena = arena.as_mut().ok_or(BuildError::BodyClosed)?;
        arena.unbound.insert(expression.clone(), value);
        Ok(value)
    }

    pub(super) fn load(&self, ty: Type, site: Site) -> Result<usize, BuildError> {
        self.with_open(|table| {
            // Two reads of the same address may observe different stores. Each load
            // therefore gets its own value instead of entering the expression cache.
            table.push(Value {
                ty,
                definition: ValueDefinition::Load { site },
            })
        })
    }

    pub(super) fn operation_result(
        &self,
        ty: Type,
        site: Site,
        component: usize,
    ) -> Result<usize, BuildError> {
        self.with_open(|table| {
            table.push(Value {
                ty,
                definition: ValueDefinition::OperationResult { site, component },
            })
        })
    }

    pub(super) fn join_result(
        &self,
        ty: Type,
        site: Site,
        component: usize,
        inputs: &[usize],
    ) -> Result<usize, BuildError> {
        self.with_open(|table| table.join_result(ty, site, component, inputs))
    }

    pub(super) fn child_scope(&self, parent: usize) -> Result<usize, BuildError> {
        let mut arena = self.0.borrow_mut();
        let arena = arena.as_mut().ok_or(BuildError::BodyClosed)?;
        let scope = arena.scopes.len();
        arena.scopes.push(parent);
        Ok(scope)
    }

    pub(super) fn definition_scope(&self, value: usize) -> Result<usize, BuildError> {
        let arena = self.0.borrow();
        let arena = arena.as_ref().ok_or(BuildError::BodyClosed)?;
        // Calculated handles keep their original operand scopes separately,
        // including dependencies that disappeared during folding.
        Ok(match arena.table.values[value].definition {
            ValueDefinition::Parameter(_) => 0,
            ValueDefinition::LoopInput { block, .. } => block,
            ValueDefinition::Load { site }
            | ValueDefinition::OperationResult { site, .. }
            | ValueDefinition::JoinResult { site, .. } => site.block,
            ValueDefinition::Constant(_) | ValueDefinition::Expression(_) => {
                unreachable!("calculated handles retain their original operand scopes")
            }
        })
    }

    pub(super) fn merge_scopes(
        &self,
        left: Option<usize>,
        right: Option<usize>,
    ) -> Result<Option<usize>, BuildError> {
        let arena = self.0.borrow();
        let arena = arena.as_ref().ok_or(BuildError::BodyClosed)?;
        Ok(arena.merge_scopes(left, right))
    }

    pub(super) fn require_visible(
        &self,
        required: Option<usize>,
        scope: usize,
    ) -> Result<(), BuildError> {
        let arena = self.0.borrow();
        let arena = arena.as_ref().ok_or(BuildError::BodyClosed)?;
        if required.is_some_and(|owner| arena.contains(owner, scope)) {
            Ok(())
        } else {
            Err(BuildError::OutOfScope)
        }
    }

    pub(super) fn require_scope(&self, owner: usize, scope: usize) -> Result<(), BuildError> {
        let arena = self.0.borrow();
        let arena = arena.as_ref().ok_or(BuildError::BodyClosed)?;
        if arena.contains(owner, scope) {
            Ok(())
        } else {
            Err(BuildError::OutOfScope)
        }
    }

    pub(super) fn expression(
        &self,
        ty: Type,
        expression: Expression<usize>,
    ) -> Result<usize, BuildError> {
        self.with_open(|table| table.expression(ty, expression))
    }

    pub(super) fn normalize(&self, input: usize) -> Result<usize, BuildError> {
        self.with_open(|table| table.normalize(input))
    }

    fn with_open(&self, build: impl FnOnce(&mut ValueTable) -> usize) -> Result<usize, BuildError> {
        let mut arena = self.0.borrow_mut();
        Ok(build(
            &mut arena.as_mut().ok_or(BuildError::BodyClosed)?.table,
        ))
    }

    /// Close construction and transfer the table. Scope and admission caches
    /// are released here, even while the returned values remain in use.
    pub(super) fn take(&self) -> Option<ValueTable> {
        let arena = self.0.borrow_mut().take();
        arena.map(|arena| arena.table)
    }
}

impl Construction {
    fn contains(&self, owner: usize, mut scope: usize) -> bool {
        while scope != owner && scope != 0 {
            scope = self.scopes[scope];
        }
        scope == owner
    }

    fn merge_scopes(&self, left: Option<usize>, right: Option<usize>) -> Option<usize> {
        let left = left?;
        let right = right?;
        if self.contains(left, right) {
            Some(right)
        } else if self.contains(right, left) {
            Some(left)
        } else {
            None
        }
    }
}
