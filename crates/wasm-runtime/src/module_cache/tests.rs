use super::*;
use crate::{Error, Result};
use std::{
    cell::Cell,
    future::{pending, poll_fn},
    pin::pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll, Wake, Waker},
};

const ASSET: &[u8] = b"embedded asset";

fn ready<F: Future>(future: F) -> F::Output {
    match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("expected a completed request"),
    }
}

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn concurrent_requests_share_compilation_and_wake_waiters() {
    let cache = ModuleCache::default();
    let finished = Cell::new(false);
    let mut first = pin!(cache.get_or_compile::<Error, _>(ASSET, || poll_fn(|_| {
        if finished.get() {
            Poll::Ready(Ok::<_, Error>(Arc::new(42)))
        } else {
            Poll::Pending
        }
    })));
    assert!(first
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());

    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut second = pin!(cache.get_or_compile::<Error, _>(ASSET, || async {
        panic!("the waiting request must reuse the first module")
    }));
    assert!(second
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_pending());

    finished.set(true);
    let first = ready(first).unwrap();
    assert!(wakes.0.load(Ordering::SeqCst) > 0);
    let second = ready(second).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert!(Arc::ptr_eq(
        &first,
        &ready(cache.get_or_compile::<Error, _>(ASSET, || async {
            panic!("the completed module must be reused")
        }))
        .unwrap(),
    ));
}

#[test]
fn different_artifact_keys_compile_independently() {
    let cache = ModuleCache::default();
    let mut first = pin!(cache.get_or_compile::<Error, _>(ASSET, pending::<Result<u32>>));
    assert!(first
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());

    // Different configuration keys can complete while another request is pending.
    let prefix = &ASSET[..3];
    assert_eq!(
        ready(cache.get_or_compile::<Error, _>(prefix, || async { Ok::<_, Error>(7) })).unwrap(),
        7,
    );
}

#[test]
fn failed_compilation_allows_retry() {
    let cache = ModuleCache::default();
    assert!(ready(cache.get_or_compile::<Error, _>(ASSET, || async {
        Err::<u32, _>(Error::new("compile", "invalid module"))
    }))
    .is_err());
    assert_eq!(
        ready(cache.get_or_compile::<Error, _>(ASSET, || async { Ok::<_, Error>(42) })).unwrap(),
        42,
    );
}

#[test]
fn cancelling_compilation_wakes_a_waiting_request_to_retry() {
    let cache = ModuleCache::default();
    let mut first = Box::pin(cache.get_or_compile::<Error, _>(ASSET, pending::<Result<u32>>));
    assert!(first
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());

    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut second = pin!(cache.get_or_compile::<Error, _>(ASSET, || async { Ok::<_, Error>(42) }));
    assert!(second
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_pending());

    drop(first);
    assert!(wakes.0.load(Ordering::SeqCst) > 0);
    assert_eq!(ready(second).unwrap(), 42);
}

#[test]
fn separate_caches_do_not_share_modules() {
    let first = ModuleCache::default();
    let second = ModuleCache::default();
    let first =
        ready(first.get_or_compile::<Error, _>(ASSET, || async { Ok::<_, Error>(Arc::new(42)) }))
            .unwrap();
    let second =
        ready(second.get_or_compile::<Error, _>(ASSET, || async { Ok::<_, Error>(Arc::new(42)) }))
            .unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
}

#[test]
fn equivalent_owned_keys_share_a_compiled_module() {
    let first_key = b"generator-v1/profile-a".to_vec();
    let second_key = first_key.clone();
    let cache = ModuleCache::default();
    let first = ready(
        cache.get_or_compile::<Error, _>(&first_key, || async { Ok::<_, Error>(Arc::new(42)) }),
    )
    .unwrap();
    let second = ready(
        cache.get_or_compile::<Error, _>(&second_key, || async { Ok::<_, Error>(Arc::new(42)) }),
    )
    .unwrap();
    assert!(Arc::ptr_eq(&first, &second));
}
