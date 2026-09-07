use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::{
    control::Site,
    integer::{self, BinaryOp, CompareOp, ShiftOp},
    memory::Location,
    BuildError, Type, Value, ValueKind,
};

#[derive(Clone)]
pub(super) struct ExpressionArena(Rc<RefCell<Option<ValueArena>>>);

#[derive(Default)]
struct ValueArena {
    values: Vec<Value>,
    interned: HashMap<Value, usize>,
    unsigned_bits: Vec<u8>,
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
                kind: ValueKind::Load { location, site },
            })
        })
    }

    pub(super) fn call_result(&self, ty: Type, site: Site) -> Result<usize, BuildError> {
        self.with_open(|arena| {
            arena.push(Value {
                ty,
                kind: ValueKind::CallResult { site },
            })
        })
    }

    pub(super) fn join_result(
        &self,
        ty: Type,
        site: Site,
        inputs: &[usize],
    ) -> Result<usize, BuildError> {
        self.with_open(|arena| {
            // Joining does not clear upper bits. Later observers use the largest
            // bound from the arms that actually yield a value.
            let bits = inputs
                .iter()
                .map(|&id| arena.unsigned_bits[id])
                .max()
                .unwrap();
            arena.push_with_bits(
                Value {
                    ty,
                    kind: ValueKind::JoinResult { site },
                },
                bits,
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

    pub(super) fn require_visible(&self, value: usize, scope: usize) -> Result<(), BuildError> {
        let arena = self.0.borrow();
        let arena = arena.as_ref().ok_or(BuildError::BodyClosed)?;
        if arena.availability[value].is_some_and(|owner| arena.contains(owner, scope)) {
            Ok(())
        } else {
            Err(BuildError::OutOfScope)
        }
    }

    pub(super) fn binary(
        &self,
        operator: BinaryOp,
        left: usize,
        right: usize,
    ) -> Result<usize, BuildError> {
        self.with_open(|arena| arena.binary(operator, left, right))
    }

    pub(super) fn shift(
        &self,
        operator: ShiftOp,
        input: usize,
        count: u32,
    ) -> Result<usize, BuildError> {
        self.with_open(|arena| {
            let value = arena.values[input];
            let effective = integer::shift_count(value.ty, count);
            if effective == 0 {
                return input;
            }
            if let ValueKind::Constant(bits) = value.kind {
                let bits = match operator {
                    ShiftOp::Left => bits.wrapping_shl(effective),
                    ShiftOp::Right => bits >> effective,
                };
                return arena.constant(value.ty, bits);
            }
            let input = match operator {
                ShiftOp::Left => input,
                ShiftOp::Right => arena.normalize(input),
            };
            arena.intern(Value {
                ty: value.ty,
                kind: ValueKind::Shift(operator, input, count),
            })
        })
    }

    pub(super) fn compare(
        &self,
        operator: CompareOp,
        left: usize,
        right: usize,
    ) -> Result<usize, BuildError> {
        self.with_open(|arena| arena.compare(operator, left, right))
    }

    pub(super) fn convert(&self, input: usize, target: Type) -> Result<usize, BuildError> {
        self.with_open(|arena| {
            let source = arena.values[input];
            if source.ty == target {
                return input;
            }
            if let ValueKind::Constant(bits) = source.kind {
                return arena.constant(target, bits);
            }
            let input = if source.ty.bits() < target.bits() {
                arena.normalize(input)
            } else {
                input
            };
            arena.intern(Value {
                ty: target,
                kind: ValueKind::Convert(input),
            })
        })
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
}

impl ValueArena {
    fn constant(&mut self, ty: Type, bits: u64) -> usize {
        self.intern(Value {
            ty,
            kind: ValueKind::Constant(ty.normalize(bits)),
        })
    }

    fn binary(&mut self, operator: BinaryOp, left: usize, right: usize) -> usize {
        let a = self.values[left];
        let b = self.values[right];
        debug_assert_eq!(a.ty, b.ty);
        if let (ValueKind::Constant(a), ValueKind::Constant(b)) = (a.kind, b.kind) {
            return self.constant(
                self.values[left].ty,
                match operator {
                    BinaryOp::Add => a.wrapping_add(b),
                    BinaryOp::And => a & b,
                    BinaryOp::Or => a | b,
                    BinaryOp::Xor => a ^ b,
                },
            );
        }
        match (operator, a.kind, b.kind) {
            (BinaryOp::Add | BinaryOp::Or | BinaryOp::Xor, _, ValueKind::Constant(0)) => left,
            (BinaryOp::Add | BinaryOp::Or | BinaryOp::Xor, ValueKind::Constant(0), _) => right,
            (BinaryOp::And | BinaryOp::Or, _, _) if left == right => left,
            (BinaryOp::And, _, ValueKind::Constant(bits)) if bits == a.ty.mask() => left,
            (BinaryOp::And, ValueKind::Constant(bits), _) if bits == a.ty.mask() => right,
            (BinaryOp::And, _, ValueKind::Constant(0))
            | (BinaryOp::And, ValueKind::Constant(0), _) => self.constant(a.ty, 0),
            (BinaryOp::Or, _, ValueKind::Constant(bits)) if bits == a.ty.mask() => right,
            (BinaryOp::Or, ValueKind::Constant(bits), _) if bits == a.ty.mask() => left,
            _ => self.intern(Value {
                ty: a.ty,
                kind: ValueKind::Binary(operator, left, right),
            }),
        }
    }

    fn compare(&mut self, operator: CompareOp, left: usize, right: usize) -> usize {
        let a = self.values[left];
        let b = self.values[right];
        debug_assert_eq!(a.ty, b.ty);
        if let (ValueKind::Constant(a), ValueKind::Constant(b)) = (a.kind, b.kind) {
            let result = match operator {
                CompareOp::Eq => a == b,
                CompareOp::Ne => a != b,
                CompareOp::Lt => a < b,
                CompareOp::Ge => a >= b,
            };
            return self.constant(Type::I1, u64::from(result));
        }
        if left == right {
            return self.constant(
                Type::I1,
                u64::from(matches!(operator, CompareOp::Eq | CompareOp::Ge)),
            );
        }
        if matches!(operator, CompareOp::Eq | CompareOp::Ne) {
            let input = match (a.kind, b.kind) {
                (_, ValueKind::Constant(0)) => Some(left),
                (ValueKind::Constant(0), _) => Some(right),
                _ => None,
            };
            if let Some(input) = input {
                if operator == CompareOp::Ne && a.ty == Type::I1 {
                    return input;
                }
                return self.zero_test(input, operator == CompareOp::Ne);
            }
            if self.unsigned_bits[left] > a.ty.bits() && self.unsigned_bits[right] > a.ty.bits() {
                // Compare the low-bit difference once instead of masking both operands.
                let difference = self.binary(BinaryOp::Xor, left, right);
                return self.zero_test(difference, operator == CompareOp::Ne);
            }
        }
        let left = self.normalize(left);
        let right = self.normalize(right);
        self.intern(Value {
            ty: Type::I1,
            kind: ValueKind::Compare(operator, left, right),
        })
    }

    fn zero_test(&mut self, input: usize, nonzero: bool) -> usize {
        let input = self.normalize(input);
        self.intern(Value {
            ty: Type::I1,
            kind: ValueKind::ZeroTest { input, nonzero },
        })
    }

    fn normalize(&mut self, input: usize) -> usize {
        let value = self.values[input];
        if self.unsigned_bits[input] <= value.ty.bits() {
            return input;
        }
        // Calls, returns and unsigned observations share the masked result.
        // Arithmetic and stores keep the original value.
        self.intern(Value {
            ty: value.ty,
            kind: ValueKind::Normalize(input),
        })
    }

    fn contains(&self, owner: usize, mut scope: usize) -> bool {
        while scope != owner && scope != 0 {
            scope = self.scopes[scope];
        }
        scope == owner
    }

    // Pure expressions may be built anywhere, but consuming them requires every
    // read, call or join dependency to be visible. Sibling results have no such scope.
    fn availability(&self, value: Value) -> Option<usize> {
        match value.kind {
            ValueKind::Constant(_) | ValueKind::Parameter(_) => Some(0),
            ValueKind::Load { site, .. }
            | ValueKind::CallResult { site }
            | ValueKind::JoinResult { site } => Some(site.region),
            ValueKind::Binary(_, a, b) | ValueKind::Compare(_, a, b) => {
                let a = self.availability[a]?;
                let b = self.availability[b]?;
                if self.contains(a, b) {
                    Some(b)
                } else if self.contains(b, a) {
                    Some(a)
                } else {
                    None
                }
            }
            ValueKind::Shift(_, input, _)
            | ValueKind::Convert(input)
            | ValueKind::Normalize(input)
            | ValueKind::ZeroTest { input, .. } => self.availability[input],
        }
    }

    fn push(&mut self, value: Value) -> usize {
        let bits = integer::unsigned_bits(value, &self.unsigned_bits);
        self.push_with_bits(value, bits)
    }

    fn push_with_bits(&mut self, value: Value, bits: u8) -> usize {
        let index = self.values.len();
        self.availability.push(self.availability(value));
        self.unsigned_bits.push(bits);
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
