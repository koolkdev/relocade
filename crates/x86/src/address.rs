//! Address components decoded from bytes; register values are read only when
//! shared instruction semantics computes the linear address.

use wasm86_compiler::{BuildError, FunctionBuilder, IntoOp, Val, I1, I32};

use crate::{register::Register, state::State};

pub(super) struct RegisterTerm {
    pub(super) register: Register<I32>,
    pub(super) present: Option<Val<I1>>,
}

pub(super) struct IndexTerm<V> {
    pub(super) register: RegisterTerm,
    pub(super) shift: V,
}

pub(super) struct Address32<V> {
    pub(super) base: Option<RegisterTerm>,
    pub(super) index: Option<IndexTerm<V>>,
    pub(super) displacement: V,
}

impl RegisterTerm {
    fn read(
        self,
        body: &mut FunctionBuilder<'_>,
        state: &mut State,
    ) -> Result<Val<I32>, BuildError> {
        // Synchronize completed definitions in the parent. The load itself can
        // stay in the selected arm, without retaining child values in the cache.
        let value = state.read_register(body, self.register)?;
        match self.present {
            Some(condition) => {
                body.if_value::<I32>(condition, |arm| arm.yield_(value), |arm| arm.yield_(0))
            }
            None => Ok(value),
        }
    }
}

pub(super) fn resolve<V: IntoOp<I32>>(
    body: &mut FunctionBuilder<'_>,
    state: &mut State,
    address: Address32<V>,
) -> Result<Val<I32>, BuildError> {
    let mut value = match address.base {
        Some(base) => base.read(body, state)?,
        None => body.value::<I32>(0)?,
    };
    if let Some(index) = address.index {
        let register = index.register.read(body, state)?;
        value = value.add(register.shl(index.shift));
    }
    Ok(value.add(address.displacement))
}
