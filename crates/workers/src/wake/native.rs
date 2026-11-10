use super::{Lifetime, WakeEndpoint, WakeHost, WakePeer, WakeTarget};
use std::{
    marker::PhantomData,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
    task::{Wake, Waker},
};

pub(super) type HostBinding<R> = Arc<R>;
pub(super) type Endpoint = ();
pub(super) struct Binding;

/// Retains host access to a fixed signal before starting the execution worker.
pub fn wake_connection<R: WakeTarget + Send + Sync>(target: R) -> (WakeHost<R>, WakeEndpoint<R>) {
    let target = Arc::new(target);
    let active = Arc::new(AtomicBool::new(true));
    let notification = Waker::from(Arc::new(Request {
        target: Arc::downgrade(&target),
        active: active.clone(),
    }));
    (
        WakeHost {
            _lifetime: Lifetime {
                active: active.clone(),
                on_close: None,
            },
            peer: WakePeer {
                target: (*target).clone(),
                endpoint: (),
                active: active.clone(),
            },
            _binding: target,
        },
        WakeEndpoint {
            lifetime: Lifetime {
                active,
                on_close: None,
            },
            notification,
            binding: (),
            target: PhantomData,
        },
    )
}

pub(super) fn bind<R>(_: Endpoint, _target: R, _active: Arc<AtomicBool>) -> Binding {
    Binding
}

struct Request<R> {
    target: Weak<R>,
    active: Arc<AtomicBool>,
}
impl<R: WakeTarget + Send + Sync> Wake for Request<R> {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        if self.active.load(Ordering::SeqCst) {
            if let Some(target) = self.target.upgrade() {
                target.wake();
            }
        }
    }
}
