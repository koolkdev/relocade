#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]

#[doc(hidden)]
pub mod slots;
mod types;
#[doc(hidden)]
pub mod union;
mod values;
#[cfg(target_arch = "wasm32")]
mod web;

/// Derives a portable logical layout and Rust codec for an owned struct or enum.
/// Structs may be generic; enums must be concrete. See the crate documentation
/// for field order, enum tags, payload overlap, and supported shapes.
pub use relocade_derive::WasmAbi;
pub use types::{AbiType, FunctionSignature, F32, F64, I1, I16, I32, I64, I8};
pub use values::{lift_flat, lower_flat, AbiValue, Carrier, Flat, Value, WasmRepr};
#[cfg(target_arch = "wasm32")]
pub use web::{callback, Error, Function, Result, WasmTuple};

/// The representation selected by an enum's ABI definition. Compiler integration
/// consumes its tags and payload without choosing a second representation.
#[doc(hidden)]
pub trait Enum: AbiType<Logical = (I32, Self::Payload)> {
    type Payload: slots::Slots;
    /// One distinct ABI tag per variant, in declaration order.
    const TAGS: &'static [u32];
}
