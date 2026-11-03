use super::{WakeConnection, WakeTarget};
use crate::{
    worker::{channel, Endpoint, Port, ResourceTransfer, WorkerResource},
    Error, Result,
};
use futures_channel::oneshot;
use futures_util::future::{AbortHandle, Abortable};
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Wake, Waker},
};

thread_local! {
    // Only this thread's reconstructed resources are callable here. Wakers carry
    // an ID, never a JavaScript handle or a pointer into another worker's table.
    static TARGETS: RefCell<HashMap<usize, Box<dyn Fn()>>> = RefCell::default();
}
static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

/// Owns the eager host-side resource receiver. Keep it alive through session
/// completion; it also finishes when the execution connection is dropped.
pub struct WakeHost(AbortHandle);

/// A session connection that can be transferred with `ThreadExt::run_resource`.
pub struct WakeEndpoint<R: WakeTarget> {
    id: usize,
    endpoint: Endpoint<Registration<R>>,
}

/// Installs sender-local access and retains the browser registration channel.
pub(super) struct Binding<R: WakeTarget> {
    local: LocalBinding,
    port: RefCell<Port<Registration<R>>>,
}

struct Registration<R> {
    target: R,
    ready: oneshot::Sender<()>,
}

impl<R: WakeTarget> WorkerResource for Registration<R> {
    type Posted = R::Posted;

    fn prepare(
        self,
    ) -> ResourceTransfer<Self::Posted, impl FnOnce(Self::Posted) -> Self + Send + 'static> {
        let Self { target, ready } = self;
        let ResourceTransfer { posted, rebuild } = target.prepare();
        ResourceTransfer::new(posted, move |posted| Self {
            target: rebuild(posted),
            ready,
        })
    }
}

/// Starts the host receiver immediately; polling a worker's join handle is not
/// required for connection setup. Only these two connected threads can signal
/// busy execution. Other senders still wake the executor normally.
pub fn wake_connection<R: WakeTarget>() -> Result<(WakeHost, WakeEndpoint<R>)> {
    let id = NEXT_ID
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .expect("worker wake IDs exhausted");
    let (host, endpoint) = channel::<Registration<R>>()?;
    let (stop, registration) = AbortHandle::new_pair();
    wasm_bindgen_futures::spawn_local(async move {
        let host = async move {
            let binding = LocalBinding(id);
            let mut host = host.bind();
            while let Some(Ok(Registration { target, ready })) = host.recv().await {
                binding.install(target);
                let _ = ready.send(());
            }
        };
        let _ = Abortable::new(host, registration).await;
    });
    Ok((WakeHost(stop), WakeEndpoint { id, endpoint }))
}

impl<R: WakeTarget> WorkerResource for WakeEndpoint<R> {
    type Posted = <Endpoint<R> as WorkerResource>::Posted;

    fn prepare(
        self,
    ) -> ResourceTransfer<Self::Posted, impl FnOnce(Self::Posted) -> Self + Send + 'static> {
        let Self { id, endpoint } = self;
        let ResourceTransfer { posted, rebuild } = endpoint.prepare();
        ResourceTransfer::new(posted, move |posted| Self {
            id,
            endpoint: rebuild(posted),
        })
    }
}

impl<R: WakeTarget> WakeEndpoint<R> {
    pub fn bind(self) -> WakeConnection<R> {
        WakeConnection::new(
            Binding {
                local: LocalBinding(self.id),
                port: RefCell::new(self.endpoint.bind()),
            },
            Waker::from(Arc::new(Request(self.id))),
        )
    }
}

impl<R: WakeTarget> Binding<R> {
    pub(super) async fn install(&self, target: R) -> Result<()> {
        self.local.install(target.clone());
        let (ready, installed) = oneshot::channel();
        self.port
            .borrow_mut()
            .send(Registration { target, ready })?;
        installed
            .await
            .map_err(|error| Error::new("connect worker wake", error))?;
        Ok(())
    }
}

struct LocalBinding(usize);
impl LocalBinding {
    fn install<R: WakeTarget>(&self, target: R) {
        let previous = TARGETS
            .with_borrow_mut(|targets| targets.insert(self.0, Box::new(move || target.wake())));
        drop(previous);
    }
}
impl Drop for LocalBinding {
    fn drop(&mut self) {
        let target = TARGETS.with_borrow_mut(|targets| targets.remove(&self.0));
        drop(target);
    }
}

struct Request(usize);
impl Wake for Request {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        TARGETS.with_borrow(|targets| {
            if let Some(target) = targets.get(&self.0) {
                target();
            }
        });
    }
}

impl Drop for WakeHost {
    fn drop(&mut self) {
        self.0.abort();
    }
}
