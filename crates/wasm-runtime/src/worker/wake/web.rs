use super::{Lifetime, WakeEndpoint, WakeHost, WakePeer, WakeTarget};
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
    // Each thread calls its own reconstructed resource. Wakers carry only IDs.
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
/// Other threads still need their own resource binding to signal busy execution.
pub fn wake_connection<R: WakeTarget>(target: R) -> (WakeHost<R>, WakeEndpoint<R>) {
    let id = NEXT_ID
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .expect("worker wake IDs exhausted");
    let active = Arc::new(AtomicBool::new(true));
    (
        WakeHost {
            _lifetime: Lifetime(active.clone()),
            peer: WakePeer {
                target: target.clone(),
                endpoint: id,
                active: active.clone(),
            },
            _binding: HostBinding {
                _local: bind(id, target, active.clone()),
                _target: PhantomData,
            },
        },
        WakeEndpoint {
            lifetime: Lifetime(active),
            notification: Waker::from(Arc::new(Request(id))),
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

struct Request(usize);
impl Wake for Request {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        let target = TARGETS.with_borrow(|targets| targets.get(&self.0).and_then(Weak::upgrade));
        if let Some(target) = target {
            (target.wake)();
        }
    }
}
