//! Real16 dispatch and call profiling use the same packed CS:IP identity.

use super::{Dispatch, EntryTable};
use crate::{
    abi,
    state::{Cpu, State},
    CallProfile, Segment,
};
use relocade_compiler::{BuildError, Program, TypedFunc, I32};
use std::sync::Arc;

pub(super) fn lookup(
    program: &mut Program,
    cpu: Arc<Cpu>,
    dispatch: Dispatch,
) -> TypedFunc<fn() -> u32> {
    program.function::<fn() -> u32>(move |mut body, ()| {
        let eip = cpu.read_eip(&mut body)?;
        let mut state = State::new(&cpu);
        let protected = state.protection_enabled(&mut body)?;
        // Only an eligible table miss (slot zero) can become a Real16 profiling sample.
        body.if_(protected.or(eip.unsigned().ge(0x10000)), |body| {
            body.return_(EntryTable::INTERPRETER_SLOT)
        })?;
        let cs = state
            .segments()
            .read_selector(&mut body, &Segment::Cs.into())?;
        dispatch.lookup(body, &cs.unsigned().extend::<I32>().shl(16).or(eip))
    })
}

pub(super) fn define_call_resolver(
    program: &mut Program,
    cpu: Arc<Cpu>,
    lookup: TypedFunc<fn() -> u32>,
) -> Result<TypedFunc<abi::ResolveCallSignature>, BuildError> {
    let recorder = CallProfile::declare_recorder(program);
    abi::RESOLVE_CALL.define(program, move |mut body, ()| {
        let slot = lookup.call(&mut body, ())?;
        body.if_(slot.ne(0), |body| body.return_((slot, true)))?;
        let eip = cpu.read_eip(&mut body)?;
        let mut state = State::new(&cpu);
        let cs = state
            .segments()
            .read_selector(&mut body, &Segment::Cs.into())?;
        let key = cs.unsigned().extend::<I32>().shl(16).or(eip);
        let can_continue = recorder.call(&mut body, (key,))?;
        body.return_((EntryTable::INTERPRETER_SLOT, can_continue))
    })
}
