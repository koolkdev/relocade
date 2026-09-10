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
        component: usize,
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
                    kind: ValueKind::JoinResult { site, component },
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
        count: usize,
    ) -> Result<usize, BuildError> {
        self.with_open(|arena| {
            let value = arena.values[input];
            if let ValueKind::Constant(bits) = arena.values[count].kind {
                let effective = integer::shift_count(value.ty, bits as u32);
                if effective == 0 {
                    return input;
                }
                if let ValueKind::Constant(bits) = value.kind {
                    let bits = integer::shift(value.ty, operator, bits, effective);
                    return arena.constant(value.ty, bits);
                }
            }
            if matches!(value.kind, ValueKind::Constant(0)) {
                return input;
            }
            let input = match operator {
                ShiftOp::Left => input,
                ShiftOp::RightUnsigned => arena.normalize(input),
                // Interpret the logical sign before shifting the Wasm carrier;
                // upper bits from narrow arithmetic need not be normalized.
                ShiftOp::RightSigned => arena.sign_extend(
                    input,
                    if value.ty == Type::I64 {
                        Type::I64
                    } else {
                        Type::I32
                    },
                ),
            };
            let count = if value.ty == Type::I64 {
                arena.convert(count, Type::I64)
            } else {
                count
            };
            arena.intern(Value {
                ty: value.ty,
                kind: ValueKind::Shift {
                    operator,
                    value: input,
                    count,
                },
            })
        })
    }

    pub(super) fn select(
        &self,
        condition: usize,
        when_true: usize,
        when_false: usize,
    ) -> Result<usize, BuildError> {
        self.with_open(|arena| {
            let condition = arena.normalize(condition);
            let value = Value {
                ty: arena.values[when_true].ty,
                kind: ValueKind::Select {
                    condition,
                    when_true,
                    when_false,
                },
            };
            match arena.values[condition].kind {
                ValueKind::Constant(0) => return when_false,
                ValueKind::Constant(_) => return when_true,
                _ if when_true == when_false => return when_true,
                _ => {}
            }
            arena.intern(value)
        })
    }

    pub(super) fn popcnt(&self, input: usize) -> Result<usize, BuildError> {
        self.with_open(|arena| {
            let value = arena.values[input];
            if let ValueKind::Constant(bits) = value.kind {
                return arena.constant(value.ty, integer::popcnt(bits));
            }
            let input = arena.normalize(input);
            arena.intern(Value {
                ty: value.ty,
                kind: ValueKind::Popcnt(input),
            })
        })
    }

    pub(super) fn sign_extend(&self, input: usize, target: Type) -> Result<usize, BuildError> {
        self.with_open(|arena| arena.sign_extend(input, target))
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
        self.with_open(|arena| arena.convert(input, target))
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

    fn convert(&mut self, input: usize, target: Type) -> usize {
        let source = self.values[input];
        if source.ty == target {
            return input;
        }
        if let ValueKind::Constant(bits) = source.kind {
            return self.constant(target, bits);
        }
        let input = if source.ty.bits() < target.bits() {
            self.normalize(input)
        } else {
            input
        };
        self.intern(Value {
            ty: target,
            kind: ValueKind::Convert(input),
        })
    }

    fn sign_extend(&mut self, input: usize, target: Type) -> usize {
        let source = self.values[input];
        if source.ty == target {
            return input;
        }
        if let ValueKind::Constant(bits) = source.kind {
            return self.constant(target, integer::signed_value(source.ty, bits) as u64);
        }
        self.intern(Value {
            ty: target,
            kind: ValueKind::SignExtend(input),
        })
    }

    fn binary(&mut self, operator: BinaryOp, left: usize, right: usize) -> usize {
        let a = self.values[left];
        let b = self.values[right];
        debug_assert_eq!(a.ty, b.ty);
        if let (ValueKind::Constant(a), ValueKind::Constant(b)) = (a.kind, b.kind) {
            return self.constant(self.values[left].ty, integer::binary(operator, a, b));
        }
        match (operator, a.kind, b.kind) {
            (
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Or | BinaryOp::Xor,
                _,
                ValueKind::Constant(0),
            ) => left,
            (BinaryOp::Sub | BinaryOp::Xor, _, _) if left == right => self.constant(a.ty, 0),
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
            let result = integer::compare(self.values[left].ty, operator, a, b);
            return self.constant(Type::I1, u64::from(result));
        }
        if left == right {
            return self.constant(
                Type::I1,
                u64::from(matches!(
                    operator,
                    CompareOp::Eq | CompareOp::GeUnsigned | CompareOp::GeSigned
                )),
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
            let masked = match (a.kind, b.kind) {
                (_, ValueKind::Constant(mask)) => Some((left, mask)),
                (ValueKind::Constant(mask), _) => Some((right, mask)),
                _ => None,
            };
            if let Some((input, mask)) = masked {
                if let ValueKind::Binary(BinaryOp::And, x, y) = self.values[input].kind {
                    // A one-bit mask yields either zero or that mask.
                    if mask.is_power_of_two()
                        && (self.values[x].kind == ValueKind::Constant(mask)
                            || self.values[y].kind == ValueKind::Constant(mask))
                    {
                        return self.zero_test(input, operator == CompareOp::Eq);
                    }
                }
            }
            if self.unsigned_bits[left] > a.ty.bits() && self.unsigned_bits[right] > a.ty.bits() {
                // Compare the low-bit difference once instead of masking both operands.
                let difference = self.binary(BinaryOp::Xor, left, right);
                return self.zero_test(difference, operator == CompareOp::Ne);
            }
        }
        let (left, right) = if matches!(operator, CompareOp::LtSigned | CompareOp::GeSigned) {
            // A narrow arithmetic value may have dirty upper bits. Interpret its
            // logical sign before comparing the full Wasm carriers.
            let carrier = if a.ty == Type::I64 {
                Type::I64
            } else {
                Type::I32
            };
            (
                self.sign_extend(left, carrier),
                self.sign_extend(right, carrier),
            )
        } else {
            (self.normalize(left), self.normalize(right))
        };
        self.intern(Value {
            ty: Type::I1,
            kind: ValueKind::Compare(operator, left, right),
        })
    }

    fn zero_test(&mut self, input: usize, nonzero: bool) -> usize {
        let input = self.normalize(input);
        // A value already restricted to zero or one is its own nonzero test.
        if nonzero && self.unsigned_bits[input] <= 1 {
            return self.convert(input, Type::I1);
        }
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
        match value.kind {
            ValueKind::Constant(_) | ValueKind::Parameter(_) => Some(0),
            ValueKind::Load { site, .. }
            | ValueKind::CallResult { site }
            | ValueKind::JoinResult { site, .. } => Some(site.region),
            ValueKind::Binary(_, a, b)
            | ValueKind::Compare(_, a, b)
            | ValueKind::Shift {
                value: a, count: b, ..
            } => self.merge_scopes(self.availability[a], self.availability[b]),
            ValueKind::Select {
                condition,
                when_true,
                when_false,
            } => {
                let mut scope = self.availability[condition];
                for input in [when_true, when_false] {
                    scope = self.merge_scopes(scope, self.availability[input]);
                }
                scope
            }
            ValueKind::Convert(input)
            | ValueKind::SignExtend(input)
            | ValueKind::Popcnt(input)
            | ValueKind::Normalize(input)
            | ValueKind::ZeroTest { input, .. } => self.availability[input],
        }
    }

    fn push(&mut self, value: Value) -> usize {
        let bits = integer::unsigned_bits(value, &self.values, &self.unsigned_bits);
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
