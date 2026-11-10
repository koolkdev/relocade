/// Composes posted values without requiring their Rust types to implement serde.
/// Constructed by the Transfer derive; each leaf owns its browser representation.
#[doc(hidden)]
pub struct PayloadPair<A, B>(pub A, pub B);

#[cfg(target_arch = "wasm32")]
mod web;
