//! Effective-address components with deferred register reads. Instruction
//! semantics can add a displacement before resolving the effective offset.

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I1, I16, I32};

use crate::{
    register::{Gpr32, Register},
    segment::{Segment, SegmentSelection},
    state::State,
};

/// Width of effective offsets and implicit string/count registers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AddressSize {
    Bits16,
    Bits32,
}

impl AddressSize {
    pub(crate) fn wrap(self, value: Val<I32>) -> Val<I32> {
        match self {
            Self::Bits16 => value.truncate::<I16>().unsigned().extend::<I32>(),
            Self::Bits32 => value,
        }
    }
}

#[derive(Clone)]
pub(super) struct RegisterTerm {
    pub(super) register: Register<I32>,
    pub(super) present: Option<Val<I1>>,
}

#[derive(Clone)]
pub(super) struct IndexTerm<V> {
    pub(super) register: RegisterTerm,
    pub(super) shift: V,
}

#[derive(Clone)]
pub(super) struct EffectiveAddress<V> {
    pub(super) size: AddressSize,
    pub(super) base: Option<RegisterTerm>,
    pub(super) index: Option<IndexTerm<V>>,
    pub(super) displacement: V,
}

/// A segment reference and its effective offset, before linear translation.
#[derive(Clone)]
pub(super) struct MemoryAddress<V> {
    pub(super) segment: SegmentSelection,
    pub(super) offset: EffectiveAddress<V>,
}

impl<V> EffectiveAddress<V> {
    /// Default selection depends on the encoded base, never on an index register.
    /// The 16-bit decoder represents BP as the base whenever it participates.
    pub(super) fn memory(self) -> MemoryAddress<V> {
        let segment = match &self.base {
            None => Segment::Ds.into(),
            Some(base) => match (base.register.known(), &base.present) {
                (Some(Gpr32::Esp | Gpr32::Ebp), None) => Segment::Ss.into(),
                (Some(_), None) => Segment::Ds.into(),
                _ => {
                    let stack_base = base
                        .register
                        .is(Gpr32::Esp)
                        .or(base.register.is(Gpr32::Ebp));
                    let stack_base = match &base.present {
                        Some(present) => stack_base.and(present),
                        None => stack_base,
                    };
                    SegmentSelection::AddressDefault(
                        stack_base.select(Segment::Ss as u32, Segment::Ds as u32),
                    )
                }
            },
        };
        MemoryAddress {
            segment,
            offset: self,
        }
    }
}

/// A full register value used while evaluating an address, without defining
/// architectural state. Later bindings for the same register take precedence.
pub(super) struct RegisterValue {
    pub(super) register: Gpr32,
    pub(super) value: Val<I32>,
}

impl RegisterTerm {
    fn read(
        self,
        body: &mut FunctionBuilder<'_>,
        state: &mut State<'_>,
        bindings: &[RegisterValue],
    ) -> Result<Val<I32>, BuildError> {
        // Unbound reads synchronize completed definitions in the parent. Loads
        // can stay in the presence arm without caching child-scoped values.
        let value = match self.register.known() {
            Some(register) => match bindings
                .iter()
                .rev()
                .find(|entry| entry.register == register)
            {
                Some(entry) => body.value(&entry.value)?,
                None => state.read_register(body, self.register)?,
            },
            None => {
                let mut value = state.read_register(body, self.register.clone())?;
                for entry in bindings {
                    value = self.register.is(entry.register).select(&entry.value, value);
                }
                value
            }
        };
        match self.present {
            Some(condition) => {
                body.if_value::<I32>(condition, |arm| arm.yield_(value), |arm| arm.yield_(0))
            }
            None => Ok(value),
        }
    }
}

pub(super) fn resolve<V: Into<Val<I32>>>(
    body: &mut FunctionBuilder<'_>,
    state: &mut State<'_>,
    address: EffectiveAddress<V>,
    bindings: &[RegisterValue],
) -> Result<Val<I32>, BuildError> {
    let mut value = match address.base {
        Some(base) => base.read(body, state, bindings)?,
        None => body.value::<I32>(0)?,
    };
    if let Some(index) = address.index {
        let register = index.register.read(body, state, bindings)?;
        value = value.add(register.shl(index.shift));
    }
    Ok(address.size.wrap(value.add(address.displacement)))
}
