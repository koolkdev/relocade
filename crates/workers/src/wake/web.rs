use super::{Lifetime, WakeEndpoint, WakeHost, WakePeer, WakeTarget};
#[path = "web/relay.rs"]
mod relay;
use relay::Relay;
use std::{
    cell::RefCell,
    collections::HashMap,
    marker::PhantomData,
    rc::{Rc, Weak},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    task::{Wake, Waker},
};

thread_local! {
    // Bound threads call their own reconstructed resource. Unbound callbacks
    // notify a responsive host through the shared Rust relay.
    static TARGETS: RefCell<HashMap<usize, Weak<LocalTarget>>> = RefCell::default();
}
static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

pub(super) struct HostBinding<R> {
    _local: Binding,
    _target: PhantomData<R>,
}
pub(super) type Endpoint = usize;
pub(super) struct Binding {
    _target: Rc<LocalTarget>,
}

struct LocalTarget {
    id: usize,
    wake: Box<dyn Fn()>,
}

/// Installs the host's signal immediately. Capture the endpoint alongside the
/// resource passed to the worker, then bind that thread's reconstructed handle.
/// Unbound callback threads relay notifications through this responsive host.
/// Post a peer binding when direct signal access is available.
pub fn wake_connection<R: WakeTarget>(target: R) -> (WakeHost<R>, WakeEndpoint<R>) {
    let id = NEXT_ID
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .expect("worker wake IDs exhausted");
    let active = Arc::new(AtomicBool::new(true));
    let local = bind(id, target.clone(), active.clone());
    let relay = Relay::start(Rc::downgrade(&local._target), active.clone());
    let on_close = Waker::from(relay.clone());
    (
        WakeHost {
            _lifetime: Lifetime {
                active: active.clone(),
                on_close: Some(on_close.clone()),
            },
            peer: WakePeer {
                target: target.clone(),
                endpoint: id,
                active: active.clone(),
            },
            _binding: HostBinding {
                _local: local,
                _target: PhantomData,
            },
        },
        WakeEndpoint {
            lifetime: Lifetime {
                active,
                on_close: Some(on_close),
            },
            notification: Waker::from(Arc::new(Request { id, relay })),
            binding: id,
            target: PhantomData,
        },
    )
}

pub(super) fn bind<R: WakeTarget>(id: Endpoint, target: R, active: Arc<AtomicBool>) -> Binding {
    let existing = TARGETS.with_borrow(|targets| targets.get(&id).and_then(Weak::upgrade));
    if let Some(existing) = existing {
        return Binding { _target: existing };
    }
    let target = Rc::new(LocalTarget {
        id,
        wake: Box::new(move || {
            if active.load(Ordering::SeqCst) {
                target.wake();
            }
        }),
    });
    TARGETS.with_borrow_mut(|targets| targets.insert(id, Rc::downgrade(&target)));
    Binding { _target: target }
}

impl Drop for LocalTarget {
    fn drop(&mut self) {
        TARGETS.with_borrow_mut(|targets| targets.remove(&self.id));
        // The table borrow ends before captured resources are destroyed. Their
        // destructors may wake through this connection themselves.
    }
}

struct Request {
    id: usize,
    relay: Arc<Relay>,
}
impl Wake for Request {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        let target = TARGETS.with_borrow(|targets| targets.get(&self.id).and_then(Weak::upgrade));
        if let Some(target) = target {
            (target.wake)();
        } else {
            self.relay.request();
        }
    }
}
