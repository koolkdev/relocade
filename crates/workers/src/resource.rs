use crate::Post;

/// A typed handle that can be reconstructed on another worker.
///
/// The posted part uses the existing worker transport. Reconstruction captures
/// retain ordinary `Send` Rust data without serializing it. The reconstructed
/// handle and the future using it need not be `Send`.
///
/// Derive this trait on a struct with `#[worker(resource)]` fields. Each field's
/// type owns its transfer; all other fields are retained as
/// `Send + 'static` Rust captures, without serialization.
/// Named and tuple structs use the same transfer mechanism.
/// Engine objects implement this trait at their transport boundary using [`Post`].
/// Set `#[worker(crate = relocade_workers)]` when using this crate directly;
/// the default path supports the `relocade_wasm_runtime::worker` re-export.
///
/// ```
/// use relocade_workers::WorkerResource;
///
/// #[derive(WorkerResource)]
/// #[worker(crate = relocade_workers)]
/// struct Configured<T, R> {
///     config: T,
///     #[worker(resource)]
///     backing: R,
/// }
///
/// #[derive(WorkerResource)]
/// #[worker(crate = relocade_workers)]
/// struct Handle<R>(#[worker(resource)] R);
///
/// #[derive(WorkerResource)]
/// #[worker(crate = relocade_workers)]
/// struct Pair<A, B> {
///     #[worker(resource)]
///     first: A,
///     #[worker(resource)]
///     second: B,
///     label: String,
/// }
/// # trait Configuration { type Data; }
/// # #[derive(WorkerResource)]
/// # #[worker(crate = relocade_workers)]
/// # struct Associated<C: Configuration, R> {
/// #     config: C::Data,
/// #     #[worker(resource)]
/// #     backing: R,
/// # }
/// ```
///
/// Captured fields must be safe to move to another thread:
///
/// ```compile_fail
/// use relocade_workers::WorkerResource;
///
/// #[derive(WorkerResource)]
/// #[worker(crate = relocade_workers)]
/// struct Local<R: WorkerResource> {
///     local: std::rc::Rc<()>,
///     #[worker(resource)]
///     backing: R,
/// }
/// ```
pub trait WorkerResource: Sized + 'static {
    type Posted: Post + 'static;

    fn prepare(
        self,
    ) -> ResourceTransfer<Self::Posted, impl FnOnce(Self::Posted) -> Self + Send + 'static>;
}

/// A resource and its reconstruction operation for channel delivery.
/// Private fields keep a capability's underlying storage inaccessible to callers.
pub struct ResourceTransfer<P, R> {
    pub(crate) posted: P,
    pub(crate) rebuild: R,
}

impl<P, R> ResourceTransfer<P, R> {
    /// Prepares a transfer inside the resource owner, where its fields are accessible.
    pub fn new(posted: P, rebuild: R) -> Self {
        Self { posted, rebuild }
    }

    /// Reconstructs an enclosing object around the received resource.
    pub fn map<T, U>(
        self,
        wrap: impl FnOnce(T) -> U + Send + 'static,
    ) -> ResourceTransfer<P, impl FnOnce(P) -> U + Send + 'static>
    where
        R: FnOnce(P) -> T + Send + 'static,
    {
        ResourceTransfer::new(self.posted, move |posted| wrap((self.rebuild)(posted)))
    }

    /// Transfers two resources together, preserving each reconstruction operation.
    #[allow(
        clippy::type_complexity,
        reason = "both the posted values and reconstructed resources form pairs"
    )]
    pub fn zip<Q, S, T, U>(
        self,
        other: ResourceTransfer<Q, S>,
    ) -> ResourceTransfer<(P, Q), impl FnOnce((P, Q)) -> (T, U) + Send + 'static>
    where
        R: FnOnce(P) -> T + Send + 'static,
        S: FnOnce(Q) -> U + Send + 'static,
    {
        ResourceTransfer::new((self.posted, other.posted), move |(first, second)| {
            ((self.rebuild)(first), (other.rebuild)(second))
        })
    }
}

impl<A: WorkerResource, B: WorkerResource> WorkerResource for (A, B)
where
    (A::Posted, B::Posted): Post,
{
    type Posted = (A::Posted, B::Posted);

    fn prepare(
        self,
    ) -> ResourceTransfer<Self::Posted, impl FnOnce(Self::Posted) -> Self + Send + 'static> {
        self.0.prepare().zip(self.1.prepare())
    }
}
