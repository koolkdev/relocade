use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::{
    body::{Site, Value, ValueDefinition},
    integer::{self, BitBounds, BitCountOp},
    memory::Location,
    value::UnboundExpression,
    BuildError, Expression, Type,
};

mod arithmetic;
mod comparisons;
mod paths;
mod shifts;

#[derive(Clone)]
pub(super) struct ExpressionArena(Rc<RefCell<Option<ValueArena>>>);

#[derive(Default)]
struct ValueArena {
    values: Vec<Value>,
    interned: HashMap<Value, usize>,
    unbound: HashMap<UnboundExpression, usize>,
    bounds: Vec<BitBounds>,
    scopes: Vec<usize>,
    availability: Vec<Option<usize>>,
}

impl ExpressionArena {
    pub(super) fn new() -> Self {
        Self(Rc::new(RefCell::new(Some(ValueArena {
            scopes: vec![0],
            ..ValueArena::default()
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
        self.with_open(|arena| arena.intern(value))
    }

    pub(super) fn constant(&self, ty: Type, bits: u64) -> Result<usize, BuildError> {
        self.with_open(|arena| arena.constant(ty, bits))
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

    pub(super) fn load(
        &self,
        ty: Type,
        location: Location,
        site: Site,
    ) -> Result<usize, BuildError> {
        self.with_open(|arena| {
            // Two reads of the same address may observe different stores. Each load
            // therefore gets its own value instead of entering the expression cache.
            arena.push(Value {
                ty,
                definition: ValueDefinition::Load { location, site },
            })
        })
    }

    pub(super) fn operation_result(
        &self,
        ty: Type,
        site: Site,
        component: usize,
    ) -> Result<usize, BuildError> {
        self.with_open(|arena| {
            arena.push(Value {
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
        self.with_open(|arena| {
            // Joining does not clear upper bits. Later observers use the largest
            // bound from the arms that actually yield a value.
            let bounds = inputs
                .iter()
                .map(|&id| arena.bounds[id])
                .reduce(BitBounds::union)
                .unwrap();
            arena.push_with_bounds(
                Value {
                    ty,
                    definition: ValueDefinition::JoinResult { site, component },
                },
                bounds,
            )
        })
    }

    pub(super) fn child_scope(&self, parent: usize) -> Result<usize, BuildError> {
        self.with_open(|arena| {
            let scope = arena.scopes.len();
            arena.scopes.push(parent);
            scope
        })
    }

    pub(super) fn required_scope(&self, value: usize) -> Result<Option<usize>, BuildError> {
        let arena = self.0.borrow();
        let arena = arena.as_ref().ok_or(BuildError::BodyClosed)?;
        Ok(arena.availability[value])
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
        self.with_open(|arena| arena.expression(ty, expression))
    }

    pub(super) fn normalize(&self, input: usize) -> Result<usize, BuildError> {
        self.with_open(|arena| arena.normalize(input))
    }

    fn with_open(&self, build: impl FnOnce(&mut ValueArena) -> usize) -> Result<usize, BuildError> {
        let mut arena = self.0.borrow_mut();
        Ok(build(arena.as_mut().ok_or(BuildError::BodyClosed)?))
    }

    pub(super) fn take(&self) -> Option<Vec<Value>> {
        let arena = self.0.borrow_mut().take();
        arena.map(|arena| arena.values)
    }

    pub(super) fn simplify_paths(&self, block: &mut crate::body::Block) -> Result<(), BuildError> {
        let mut arena = self.0.borrow_mut();
        paths::simplify(arena.as_mut().ok_or(BuildError::BodyClosed)?, block);
        Ok(())
    }
}

impl ValueArena {
    // These inputs have logical types. Existing canonical body nodes may carry
    // wider operands and must retain their own rebuilding rules.
    fn expression(&mut self, ty: Type, expression: Expression<usize>) -> usize {
        match expression {
            Expression::Binary {
                operator,
                left,
                right,
            } => self.binary(operator, left, right),
            Expression::Compare {
                operator,
                left,
                right,
            } => self.compare(operator, left, right),
            Expression::Shift {
                operator,
                value,
                count,
            } => self.shift(operator, value, count),
            Expression::Rotate {
                operator,
                value,
                count,
            } => self.rotate(operator, value, count),
            Expression::Select {
                condition,
                when_true,
                when_false,
            } => self.select(condition, when_true, when_false),
            Expression::SignExtend { input } => self.sign_extend(input, ty),
            Expression::BitCount { operator, input } => self.bit_count(operator, input),
            Expression::ZeroTest { input, nonzero } => self.zero_test(input, nonzero),
            Expression::Convert { input } => self.convert(input, ty),
            Expression::Normalize { input } => self.normalize(input),
        }
    }

    fn select(&mut self, condition: usize, when_true: usize, when_false: usize) -> usize {
        let condition = self.normalize(condition);
        match self.values[condition].definition {
            ValueDefinition::Constant(0) => return when_false,
            ValueDefinition::Constant(_) => return when_true,
            _ if when_true == when_false => return when_true,
            _ => {}
        }
        self.intern(Value {
            ty: self.values[when_true].ty,
            definition: ValueDefinition::Expression(Expression::Select {
                condition,
                when_true,
                when_false,
            }),
        })
    }

    fn bit_count(&mut self, operator: BitCountOp, input: usize) -> usize {
        let value = self.values[input];
        if let ValueDefinition::Constant(bits) = value.definition {
            return self.constant(value.ty, integer::bit_count(value.ty, operator, bits));
        }
        let input = self.normalize(input);
        self.intern(Value {
            ty: value.ty,
            definition: ValueDefinition::Expression(Expression::BitCount { operator, input }),
        })
    }

    fn constant(&mut self, ty: Type, bits: u64) -> usize {
        self.intern(Value {
            ty,
            definition: ValueDefinition::Constant(ty.normalize(bits)),
        })
    }

    fn convert(&mut self, input: usize, target: Type) -> usize {
        let source = self.values[input];
        if source.ty == target {
            return input;
        }
        if let ValueDefinition::Constant(bits) = source.definition {
            return self.constant(target, bits);
        }
        let input = if source.ty.bits() < target.bits() {
            self.normalize(input)
        } else {
            input
        };
        self.intern(Value {
            ty: target,
            definition: ValueDefinition::Expression(Expression::Convert { input }),
        })
    }

    fn sign_extend(&mut self, input: usize, target: Type) -> usize {
        let source = self.values[input];
        if source.ty == target {
            return input;
        }
        if let ValueDefinition::Constant(bits) = source.definition {
            return self.constant(target, integer::signed_value(source.ty, bits) as u64);
        }
        let canonical = self.bounds[input].signed <= source.ty.bits();
        if canonical && target != Type::I64 {
            // Preserve the existing signed representation and its sharing when
            // only the logical type widens; unsigned convert() would mask it.
            return self.intern(Value {
                ty: target,
                definition: ValueDefinition::Expression(Expression::Convert { input }),
            });
        }
        if canonical {
            let alias = if source.ty == Type::I32 {
                input
            } else {
                self.intern(Value {
                    ty: Type::I32,
                    definition: ValueDefinition::Expression(Expression::Convert { input }),
                })
            };
            // Crossing into i64 still needs the signed carrier extension.
            return self.intern(Value {
                ty: target,
                definition: ValueDefinition::Expression(Expression::SignExtend { input: alias }),
            });
        }
        self.intern(Value {
            ty: target,
            definition: ValueDefinition::Expression(Expression::SignExtend { input }),
        })
    }

    fn sign_extend_carrier(&mut self, input: usize) -> usize {
        // Interpret the logical sign before a signed carrier operation; narrow
        // arithmetic can leave upper bits that do not belong to the value.
        let carrier = if self.values[input].ty == Type::I64 {
            Type::I64
        } else {
            Type::I32
        };
        self.sign_extend(input, carrier)
    }

    fn normalize(&mut self, input: usize) -> usize {
        let value = self.values[input];
        if self.bounds[input].unsigned <= value.ty.bits() {
            return input;
        }
        // Calls, returns and unsigned observations share the masked result.
        // Arithmetic and stores keep the original value.
        self.intern(Value {
            ty: value.ty,
            definition: ValueDefinition::Expression(Expression::Normalize { input }),
        })
    }

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

    // Scope of dependencies remaining in the runtime node. Handle provenance
    // separately retains requirements discarded by folding.
    fn availability(&self, value: Value) -> Option<usize> {
        match value.definition {
            ValueDefinition::Constant(_) | ValueDefinition::Parameter(_) => Some(0),
            ValueDefinition::LoopInput { block, .. } => Some(block),
            ValueDefinition::Load { site, .. }
            | ValueDefinition::OperationResult { site, .. }
            | ValueDefinition::JoinResult { site, .. } => Some(site.block),
            ValueDefinition::Expression(expression) => {
                expression.inputs().fold(Some(0), |scope, input| {
                    self.merge_scopes(scope, self.availability[*input])
                })
            }
        }
    }

    fn push(&mut self, value: Value) -> usize {
        let bounds = BitBounds::for_value(value, &self.values, &self.bounds);
        self.push_with_bounds(value, bounds)
    }

    fn push_with_bounds(&mut self, value: Value, bounds: BitBounds) -> usize {
        let index = self.values.len();
        self.availability.push(self.availability(value));
        self.bounds.push(bounds);
        self.values.push(value);
        index
    }

    fn intern(&mut self, value: Value) -> usize {
        if let Some(&index) = self.interned.get(&value) {
            return index;
        }
        let index = self.push(value);
        self.interned.insert(value, index);
        index
    }
}
