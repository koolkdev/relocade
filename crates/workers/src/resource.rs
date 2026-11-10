use crate::Post;

/// A typed handle that can be reconstructed on another worker.
///
/// The posted part uses the existing worker transport. Reconstruction captures
/// retain ordinary `Send` Rust data without serializing it. The reconstructed
/// handle and the future using it need not be `Send`.
///
/// Derive this trait on a struct with one `#[worker(post)]` leaf or
/// `#[worker(resource)]` nested resource field. All other
/// fields are retained as `Send + 'static` Rust captures, without serialization.
/// Named and tuple structs use the same transfer mechanism.
/// Set `#[worker(crate = relocade_workers)]` when using this crate directly;
/// the default path supports the `relocade_wasm_runtime::worker` re-export.
///
/// ```
/// use relocade_workers::WorkerResource;
///
/// #[derive(WorkerResource)]
/// #[worker(crate = relocade_workers)]
/// struct Configured<T, P> {
///     config: T,
///     #[worker(post)]
///     backing: P,
/// }
///
/// #[derive(WorkerResource)]
/// #[worker(crate = relocade_workers)]
/// struct Handle(#[worker(post)] ());
/// # trait Configuration { type Data; }
/// # #[derive(WorkerResource)]
/// # #[worker(crate = relocade_workers)]
/// # struct Associated<C: Configuration> {
/// #     config: C::Data,
/// #     #[worker(post)]
/// #     backing: (),
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
/// struct Local {
///     local: std::rc::Rc<()>,
///     #[worker(post)]
///     backing: (),
/// }
/// ```
pub trait WorkerResource: Sized + 'static {
    type Posted: Post + 'static;

    fn prepare(
        self,
    ) -> ResourceTransfer<Self::Posted, impl FnOnce(Self::Posted) -> Self + Send + 'static>;
}

/// A resource and its reconstruction operation, shared by task inputs and
/// resource channels.
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

    /// Reconstructs an enclosing object without exposing the nested resource's
    /// posted representation or moving its local state across threads.
    pub fn map<T, U>(
        self,
        wrap: impl FnOnce(T) -> U + Send + 'static,
    ) -> ResourceTransfer<P, impl FnOnce(P) -> U + Send + 'static>
    where
        R: FnOnce(P) -> T + Send + 'static,
    {
        ResourceTransfer::new(self.posted, move |posted| wrap((self.rebuild)(posted)))
    }
}

impl WorkerResource for () {
    type Posted = ();

    fn prepare(self) -> ResourceTransfer<(), impl FnOnce(()) + Send> {
        ResourceTransfer::new((), |()| ())
    }
}

impl<R: WorkerResource, E: Send + 'static> WorkerResource for Result<R, E>
where
    Result<R::Posted, ()>: Post,
{
    type Posted = Result<R::Posted, ()>;

    fn prepare(
        self,
    ) -> ResourceTransfer<Self::Posted, impl FnOnce(Self::Posted) -> Self + Send + 'static> {
        let (posted, rebuild) = match self {
            Ok(resource) => {
                let ResourceTransfer { posted, rebuild } = resource.prepare();
                (Ok(posted), Ok(rebuild))
            }
            Err(error) => (Err(()), Err(error)),
        };
        ResourceTransfer::new(posted, move |posted| match (posted, rebuild) {
            (Ok(posted), Ok(rebuild)) => Ok(rebuild(posted)),
            (Err(()), Err(error)) => Err(error),
            _ => unreachable!("resource result and reconstruction have the same variant"),
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
        let a = self.0.prepare();
        let b = self.1.prepare();
        ResourceTransfer::new((a.posted, b.posted), move |(a_posted, b_posted)| {
            ((a.rebuild)(a_posted), (b.rebuild)(b_posted))
        })
    }
}
