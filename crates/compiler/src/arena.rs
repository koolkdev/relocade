use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::{memory::Location, BuildError, Type, Value, ValueKind};

#[derive(Clone)]
pub(super) struct ExpressionArena(Rc<RefCell<Option<ValueArena>>>);

#[derive(Default)]
struct ValueArena {
    values: Vec<Value>,
    interned: HashMap<Value, usize>,
}

impl ExpressionArena {
    pub(super) fn new() -> Self {
        Self(Rc::new(RefCell::new(Some(ValueArena::default()))))
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
        let mut arena = self.0.borrow_mut();
        Ok(arena.as_mut().ok_or(BuildError::BodyClosed)?.intern(value))
    }

    pub(super) fn constant(&self, ty: Type, bits: u64) -> Result<usize, BuildError> {
        self.intern(Value {
            ty,
            kind: ValueKind::Constant(ty.normalize(bits)),
        })
    }

    pub(super) fn load(
        &self,
        ty: Type,
        location: Location,
        site: usize,
    ) -> Result<usize, BuildError> {
        let mut arena = self.0.borrow_mut();
        let arena = arena.as_mut().ok_or(BuildError::BodyClosed)?;
        // Two reads of the same address may observe different stores. Each load
        // therefore gets its own value instead of entering the expression cache.
        let index = arena.values.len();
        arena.values.push(Value {
            ty,
            kind: ValueKind::Load { location, site },
        });
        Ok(index)
    }

    pub(super) fn add(&self, left: usize, right: usize) -> Result<usize, BuildError> {
        let mut arena = self.0.borrow_mut();
        let arena = arena.as_mut().ok_or(BuildError::BodyClosed)?;
        let a = arena.values[left];
        let b = arena.values[right];
        debug_assert_eq!(a.ty, b.ty);
        Ok(match (a.kind, b.kind) {
            (ValueKind::Constant(left), ValueKind::Constant(right)) => arena.intern(Value {
                ty: a.ty,
                kind: ValueKind::Constant(a.ty.normalize(left.wrapping_add(right))),
            }),
            (_, ValueKind::Constant(0)) => left,
            (ValueKind::Constant(0), _) => right,
            _ => arena.intern(Value {
                ty: a.ty,
                kind: ValueKind::Add(left, right),
            }),
        })
    }

    pub(super) fn normalize(&self, input: usize) -> Result<usize, BuildError> {
        let mut arena = self.0.borrow_mut();
        let arena = arena.as_mut().ok_or(BuildError::BodyClosed)?;
        let value = arena.values[input];
        match value.kind {
            ValueKind::Constant(_)
            | ValueKind::Parameter(_)
            | ValueKind::Load { .. }
            | ValueKind::Normalize(_) => Ok(input),
            ValueKind::Add(..) => {
                if matches!(value.ty, Type::I1 | Type::I8 | Type::I16) {
                    // Calls and returns need unused upper bits cleared. Share that
                    // masked result while stores and arithmetic keep the original.
                    Ok(arena.intern(Value {
                        ty: value.ty,
                        kind: ValueKind::Normalize(input),
                    }))
                } else {
                    Ok(input)
                }
            }
        }
    }

    pub(super) fn take(&self) -> Option<Vec<Value>> {
        let arena = self.0.borrow_mut().take();
        arena.map(|arena| arena.values)
    }
}

impl ValueArena {
    fn intern(&mut self, value: Value) -> usize {
        *self.interned.entry(value).or_insert_with(|| {
            let index = self.values.len();
            self.values.push(value);
            index
        })
    }
}
