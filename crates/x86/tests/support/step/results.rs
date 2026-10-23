//! Translate typed fixture results at the raw Wasmtime observation boundary.

use super::{Argument, Outcome};
use relocade_compiler::{
    abi::{self, AbiValue},
    BlockBuilder, BuildError, Val, I64,
};
use relocade_wasm_bindings::slots::{Cons, Layout, Scalar, Slots};

pub(crate) trait ObservedSlots: Slots {
    fn encode(value: Self::Carrier, output: &mut Vec<Argument>);
    fn decode(values: &mut std::slice::Iter<'_, Argument>) -> Self::Carrier;
}

impl ObservedSlots for () {
    fn encode(_: (), _: &mut Vec<Argument>) {}
    fn decode(_: &mut std::slice::Iter<'_, Argument>) {}
}

pub(crate) trait ObservedScalar: Sized {
    fn encode(self) -> Argument;
    fn decode(value: &Argument) -> Self;
}

impl ObservedScalar for i32 {
    fn encode(self) -> Argument {
        Argument::I32(self)
    }
    fn decode(value: &Argument) -> Self {
        let Argument::I32(value) = value else {
            panic!("expected i32 result")
        };
        *value
    }
}
impl ObservedScalar for i64 {
    fn encode(self) -> Argument {
        Argument::I64(self)
    }
    fn decode(value: &Argument) -> Self {
        let Argument::I64(value) = value else {
            panic!("expected i64 result")
        };
        *value
    }
}

impl<H: Scalar, T: ObservedSlots> ObservedSlots for Cons<H, T>
where
    H::Carrier: ObservedScalar,
{
    fn encode(value: Self::Carrier, output: &mut Vec<Argument>) {
        output.push(value.0.encode());
        T::encode(value.1, output);
    }
    fn decode(values: &mut std::slice::Iter<'_, Argument>) -> Self::Carrier {
        (
            H::Carrier::decode(values.next().expect("result slot")),
            T::decode(values),
        )
    }
}

pub(crate) fn returned<T: AbiValue<Value = T>>(value: T) -> Outcome
where
    Layout<T>: ObservedSlots,
{
    let mut values = Vec::new();
    Layout::<T>::encode(T::lower(value), &mut values);
    Outcome::Returned(values)
}

pub(crate) fn decode<T: AbiValue>(values: &[Argument]) -> T::Value
where
    Layout<T>: ObservedSlots,
{
    let mut values = values.iter();
    let result = T::lift(Layout::<T>::decode(&mut values)).expect("valid typed result");
    assert!(values.next().is_none(), "all result slots consumed");
    result
}

/// Memory and segment probes return their value or the architectural access fault.
#[derive(
    relocade_wasm_bindings::WasmAbi, abi::CompilerValue, Clone, Copy, Debug, Eq, PartialEq,
)]
pub(crate) enum AccessResult {
    Value(u64),
    Fault(crate::Exception),
}

impl AccessResult {
    pub(crate) fn value(
        body: BlockBuilder<'_>,
        value: impl Into<Val<I64>>,
    ) -> Result<(), BuildError> {
        body.return_(AccessResultValues::value(value))
    }
    pub(crate) fn fault(
        body: BlockBuilder<'_>,
        fault: crate::ExceptionValues,
    ) -> Result<(), BuildError> {
        body.return_(AccessResultValues::fault(fault))
    }
}

impl From<u64> for AccessResult {
    fn from(value: u64) -> Self {
        Self::Value(value)
    }
}
impl From<i64> for AccessResult {
    fn from(value: i64) -> Self {
        Self::Value(value as u64)
    }
}
impl From<i32> for AccessResult {
    fn from(value: i32) -> Self {
        Self::Value(value as u64)
    }
}
