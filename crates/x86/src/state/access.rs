//! Generated CPU accesses derive locations and widths from the Rust backing layout.

use std::mem::{offset_of, size_of};

use wasm86_compiler::{BuildError, FunctionBuilder, Mem, MemoryInt, Val, I32, I8};

use crate::{
    register::{Gpr32, Register, RegisterSelection, RegisterType},
    ssa::Location,
};

use super::{CpuState, Registers};

/// Registers occupy consecutive dwords in encoding order. Indexed byte codes can
/// reach only the first four parents; named views select their parent directly.
pub(in crate::state) fn register_location<T: RegisterType>(register: Register<T>) -> Location<T> {
    let base = offset_of!(CpuState, registers) as u32;
    let stride = (size_of::<Registers>() / Gpr32::ALL.len()) as u32;
    match register.selection {
        RegisterSelection::Named { parent, byte } => {
            Location::new(base + parent as u32 * stride + byte)
        }
        RegisterSelection::Indexed { slot, byte } => {
            let displacement = slot.shl(stride.trailing_zeros());
            let displacement = match byte {
                Some(byte) => displacement.add(byte),
                None => displacement,
            };
            Location::indexed(base, T::BACKING_SLOT_COUNT * stride, displacement)
        }
    }
}

pub(in crate::state) trait CpuField {
    type Int: MemoryInt;
}

impl CpuField for u8 {
    type Int = I8;
}

impl CpuField for u32 {
    type Int = I32;
}

pub(in crate::state) fn load<T: CpuField>(
    body: &mut FunctionBuilder<'_>,
    memory: Mem,
    offset: u32,
    _field: fn(&CpuState) -> &T,
) -> Result<Val<T::Int>, BuildError> {
    body.load::<T::Int>(memory, offset)
}

pub(in crate::state) fn store<T: CpuField>(
    body: &mut FunctionBuilder<'_>,
    memory: Mem,
    offset: u32,
    _field: fn(&CpuState) -> &T,
    value: impl Into<Val<T::Int>>,
) -> Result<(), BuildError> {
    body.store::<T::Int>(memory, offset, value)
}

macro_rules! cpu_load {
    ($body:expr, $memory:expr, $($field:ident).+ $(,)?) => {
        $crate::state::access::load(
            $body,
            $memory,
            ::std::mem::offset_of!($crate::state::CpuState, $($field).+) as u32,
            |state: &$crate::state::CpuState| &state.$($field).+,
        )
    };
}

macro_rules! cpu_store {
    ($body:expr, $memory:expr, $($field:ident).+, $value:expr $(,)?) => {
        $crate::state::access::store(
            $body,
            $memory,
            ::std::mem::offset_of!($crate::state::CpuState, $($field).+) as u32,
            |state: &$crate::state::CpuState| &state.$($field).+,
            $value,
        )
    };
}

pub(in crate::state) use {cpu_load, cpu_store};
