//! Typed values that can cross a worker boundary.

/// Native messages move directly between threads.
#[cfg(not(target_arch = "wasm32"))]
pub trait Message: Send + 'static {}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + 'static> Message for T {}

/// Browser messages use structured clone. Each type owns its representation
/// and any buffer transfers; worker handling remains independent of that shape.
#[cfg(target_arch = "wasm32")]
pub trait Message: Sized + 'static {
    fn encode(self) -> crate::web::Packet;
    fn decode(value: crate::web::Value) -> crate::Result<Self>;
}
