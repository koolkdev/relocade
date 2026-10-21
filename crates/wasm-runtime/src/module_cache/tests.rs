use super::*;
use crate::Error;
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
    let mut first = pin!(cache.get_or_compile(ASSET, |_| poll_fn(|_| {
        if finished.get() {
            Poll::Ready(Ok(Arc::new(42)))
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
    let mut second = pin!(cache.get_or_compile(ASSET, |_| async {
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
        &ready(cache.get_or_compile(ASSET, |_| async {
            panic!("the completed module must be reused")
        }))
        .unwrap(),
    ));
}

#[test]
fn assets_compile_independently_and_slice_lengths_are_distinct() {
    let cache = ModuleCache::default();
    let mut first = pin!(cache.get_or_compile(ASSET, |_| pending::<Result<u32>>()));
    assert!(first
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());

    // A slice with the same address is a different asset when its length differs.
    let prefix = &ASSET[..3];
    assert_eq!(
        ready(cache.get_or_compile(prefix, |_| async { Ok(7) })).unwrap(),
        7,
    );
}

#[test]
fn failed_compilation_allows_retry() {
    let cache = ModuleCache::default();
    assert!(ready(cache.get_or_compile(ASSET, |_| async {
        Err::<u32, _>(Error::new("compile", "invalid module"))
    }))
    .is_err());
    assert_eq!(
        ready(cache.get_or_compile(ASSET, |_| async { Ok(42) })).unwrap(),
        42,
    );
}

#[test]
fn cancelling_compilation_wakes_a_waiting_request_to_retry() {
    let cache = ModuleCache::default();
    let mut first = Box::pin(cache.get_or_compile(ASSET, |_| pending::<Result<u32>>()));
    assert!(first
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());

    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut second = pin!(cache.get_or_compile(ASSET, |_| async { Ok(42) }));
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
    let first = ready(first.get_or_compile(ASSET, |_| async { Ok(Arc::new(42)) })).unwrap();
    let second = ready(second.get_or_compile(ASSET, |_| async { Ok(Arc::new(42)) })).unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
}

#[test]
fn equal_bytes_in_separate_assets_have_separate_entries() {
    static FIRST: [u8; 3] = *b"abc";
    static SECOND: [u8; 3] = *b"abc";
    let cache = ModuleCache::default();
    let first = ready(cache.get_or_compile(&FIRST, |_| async { Ok(Arc::new(42)) })).unwrap();
    let second = ready(cache.get_or_compile(&SECOND, |_| async { Ok(Arc::new(42)) })).unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
}
