use crate::Post;
use std::sync::Arc;

/// Defines how a value crosses a worker boundary.
///
/// Rust state moves without serialization. The payload uses the platform's
/// object transport, which establishes browser handles on the receiving worker.
/// Decoding combines the state with its corresponding received payload. The
/// resulting value and the future using it need not be `Send`.
///
/// Ordinary Rust values use an empty payload. Engine handles define their own
/// payload behavior in the module that owns them.
pub trait Transfer: Sized + 'static {
    /// Rust data retained without serialization.
    type State: Send + 'static;
    /// Objects passed through the existing platform transport.
    type Payload: Post + 'static;

    /// Separates this value into its corresponding state and payload.
    fn encode(self) -> (Self::State, Self::Payload);
    /// Reconstructs the value on the worker receiving its payload.
    fn decode(state: Self::State, payload: Self::Payload) -> Self;
}

macro_rules! rust_values {
    ($($ty:ty),* $(,)?) => {$(
        impl Transfer for $ty {
            type State = Self;
            type Payload = ();

            fn encode(self) -> (Self, ()) {
                (self, ())
            }

            fn decode(state: Self, (): ()) -> Self {
                state
            }
        }
    )*};
}

rust_values!(
    (),
    bool,
    char,
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    f32,
    f64,
    String,
    &'static str,
);

/// Retains the same Rust allocation; the referent does not need `Transfer`.
impl<T: ?Sized + Send + Sync + 'static> Transfer for Arc<T> {
    type State = Self;
    type Payload = ();

    fn encode(self) -> (Self, ()) {
        (self, ())
    }

    fn decode(state: Self, (): ()) -> Self {
        state
    }
}
