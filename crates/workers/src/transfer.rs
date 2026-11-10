use crate::Post;
use std::sync::Arc;

mod payload;
pub use payload::PayloadPair;

/// Defines how a value crosses a worker boundary.
///
/// Rust state moves without serialization. The payload uses the platform's
/// object transport, which establishes browser handles on the receiving worker.
/// Decoding combines the state with its corresponding received payload. The
/// resulting value and the future using it need not be `Send`.
///
/// Ordinary Rust values use an empty payload. Engine handles define their own
/// payload behavior in the module that owns them.
///
/// Deriving this trait composes every field's implementation. Named, tuple and
/// unit structs are supported. `Send` alone does not imply `Transfer`; each field
/// must implement this contract. Shared `Arc` referents need only `Send + Sync`.
/// Set `#[transfer(crate = path)]` to use a renamed dependency or a re-export.
///
/// ```
/// use relocade_workers::Transfer;
/// use std::sync::Arc;
///
/// #[derive(Transfer)]
/// struct Message<T> {
///     value: T,
///     metadata: Arc<Vec<u8>>,
/// }
///
/// #[derive(Transfer)]
/// struct Handle<T>(T);
///
/// #[derive(Transfer)]
/// struct Finished;
///
/// let (state, payload) = Finished.encode();
/// let _ = Finished::decode(state, payload);
/// let (state, payload) = Handle(42_u32).encode();
/// assert_eq!(Handle::<u32>::decode(state, payload).0, 42);
/// # trait Configuration { type Data; }
/// # #[derive(Transfer)]
/// # struct Associated<C: Configuration> { config: C::Data }
/// ```
///
/// A plain `Send` field still needs its own transfer implementation:
///
/// ```compile_fail
/// use relocade_workers::Transfer;
/// struct Plain(u32);
/// #[derive(Transfer)]
/// struct Message { value: Plain }
/// ```
///
/// Shared Rust allocations must be safe to access from another thread:
///
/// ```compile_fail
/// use relocade_workers::Transfer;
/// use std::{rc::Rc, sync::Arc};
/// #[derive(Transfer)]
/// struct Message { value: Arc<Rc<u32>> }
/// ```
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
