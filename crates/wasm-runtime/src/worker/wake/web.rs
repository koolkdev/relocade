use super::{Lifetime, WakeEndpoint, WakeHost, WakeTarget};
use std::{
    cell::RefCell,
    collections::HashMap,
    marker::PhantomData,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    task::{Wake, Waker},
};

thread_local! {
    // Each thread calls its own reconstructed resource. Wakers carry only IDs.
    static TARGETS: RefCell<HashMap<usize, Box<dyn Fn()>>> = RefCell::default();
}
static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

pub(super) struct HostBinding<R> {
    _local: Binding,
    _target: PhantomData<R>,
}
pub(super) type Endpoint = usize;
pub(super) struct Binding(usize, PhantomData<Rc<()>>);

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
    let previous = TARGETS.with_borrow_mut(|targets| {
        targets.insert(
            id,
            Box::new(move || {
                if active.load(Ordering::SeqCst) {
                    target.wake();
                }
            }),
        )
    });
    assert!(
        previous.is_none(),
        "wake endpoint must bind on its receiving thread"
    );
    Binding(id, PhantomData)
}

impl Drop for Binding {
    fn drop(&mut self) {
        let target = TARGETS.with_borrow_mut(|targets| targets.remove(&self.0));
        // Resource destructors may wake through this connection themselves.
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
