use arc_swap::ArcSwap;
use event_listener::Event;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Default)]
struct Output {
    size: [u32; 2],
    visible: bool,
}
#[derive(Default)]
struct Shared {
    output: ArcSwap<Output>,
    requested: AtomicBool,
    closed: AtomicBool,
    changed: Event,
}

/// One main-thread producer, one graphics consumer, and one pending demand bit.
/// Publication must not block: browser main cannot use atomic.wait on contention.
pub(crate) struct Redraw(Arc<Shared>);
pub(crate) struct Requests(Arc<Shared>);

pub(crate) fn channel() -> (Redraw, Requests) {
    let shared = Arc::new(Shared::default());
    (Redraw(shared.clone()), Requests(shared))
}
impl Redraw {
    pub fn output(&self, size: [u32; 2], visible: bool) {
        self.0.output.store(Arc::new(Output { size, visible }));
        self.0.changed.notify(1);
    }
    pub fn request(&self) {
        self.0.requested.store(true, Ordering::Release);
        self.0.changed.notify(1);
    }
}
impl Drop for Redraw {
    fn drop(&mut self) {
        self.0.closed.store(true, Ordering::Release);
        self.0.changed.notify(1);
    }
}
impl Requests {
    pub async fn next(&self) -> Option<[u32; 2]> {
        loop {
            let changed = self.0.changed.listen();
            if self.0.closed.load(Ordering::Acquire) {
                return None;
            }
            if self.0.requested.swap(false, Ordering::AcqRel) {
                let output = self.0.output.load();
                if output.visible && !output.size.contains(&0) {
                    return Some(output.size);
                }
                self.0.requested.store(true, Ordering::Release);
            }
            changed.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;
    use std::{
        future::Future,
        pin::pin,
        sync::atomic::AtomicUsize,
        task::{Context, Poll, Wake},
    };

    #[derive(Default)]
    struct Wakes(AtomicUsize);
    impl Wake for Wakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn waiting_consumer_is_woken_by_demand_output_and_shutdown() {
        let (redraw, requests) = channel();
        let wakes = Arc::new(Wakes::default());
        let waker = wakes.clone().into();
        let mut cx = Context::from_waker(&waker);
        let mut next = pin!(requests.next());
        assert!(next.as_mut().poll(&mut cx).is_pending());
        redraw.request();
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
        assert!(next.as_mut().poll(&mut cx).is_pending());
        redraw.output([800, 600], true);
        assert_eq!(wakes.0.load(Ordering::SeqCst), 2);
        assert_eq!(next.as_mut().poll(&mut cx), Poll::Ready(Some([800, 600])));
        let mut next = pin!(requests.next());
        assert!(next.as_mut().poll(&mut cx).is_pending());
        drop(redraw);
        assert_eq!(wakes.0.load(Ordering::SeqCst), 3);
        assert_eq!(next.as_mut().poll(&mut cx), Poll::Ready(None));
    }

    #[test]
    fn slow_consumer_takes_one_request_with_the_newest_output() {
        let (redraw, requests) = channel();
        redraw.output([640, 480], true);
        assert!(requests.next().now_or_never().is_none());
        redraw.request();
        assert_eq!(requests.next().now_or_never(), Some(Some([640, 480])));
        // One draw is now in progress. Pending demand and dimensions stay bounded.
        for width in 1..=1000 {
            redraw.output([width, 600], true);
            redraw.request();
        }
        redraw.output([800, 600], true); // Resize after the last redraw notification.
        assert_eq!(requests.next().now_or_never(), Some(Some([800, 600])));
        assert!(requests.next().now_or_never().is_none());
    }

    #[test]
    fn hidden_or_zero_size_output_waits_and_close_discards_pending_demand() {
        let (redraw, requests) = channel();
        redraw.output([640, 480], false);
        redraw.request();
        assert!(requests.next().now_or_never().is_none());
        redraw.output([0, 480], true);
        assert!(requests.next().now_or_never().is_none());
        redraw.output([320, 240], true);
        assert_eq!(requests.next().now_or_never(), Some(Some([320, 240])));
        redraw.request();
        drop(redraw);
        assert_eq!(requests.next().now_or_never(), Some(None));
    }
}
