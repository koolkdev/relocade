//! Address components decoded from bytes; register values are read only when
//! shared instruction semantics computes the linear address.

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I1, I32};

use crate::{
    register::{Gpr32, Register},
    state::State,
};

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
pub(super) struct Address32<V> {
    pub(super) base: Option<RegisterTerm>,
    pub(super) index: Option<IndexTerm<V>>,
    pub(super) displacement: V,
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
    address: Address32<V>,
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
    Ok(value.add(address.displacement))
}
