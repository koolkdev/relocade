use super::*;
use std::{
    cell::{Cell, RefCell},
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};

#[derive(Clone, Default)]
struct ManualClock(Rc<Time>);

#[derive(Default)]
struct Time {
    now: Cell<Duration>,
    waiter: RefCell<Option<Waker>>,
}

impl ManualClock {
    fn advance_to(&self, nanoseconds: u64) {
        let now = Duration::from_nanos(nanoseconds);
        assert!(now >= self.0.now.get());
        self.0.now.set(now);
        if let Some(waker) = self.0.waiter.borrow_mut().take() {
            waker.wake();
        }
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        self.0.now.get()
    }

    async fn wait_until(&self, deadline: Duration) {
        poll_fn(|cx| {
            if self.now() >= deadline {
                Poll::Ready(())
            } else {
                *self.0.waiter.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn default_rate_wakes_the_cpu_and_coalesces_late_edges_without_drift() {
    let clock = ManualClock::default();
    clock.advance_to(1_000_000_000);
    let timer = Rc::new(RefCell::new(Timer {
        clock: clock.clone(),
        pending: false,
        waker: None,
    }));
    let mut activity = pin!(Timer::run(timer.clone()));
    let mut device_cx = Context::from_waker(Waker::noop());
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut cpu_cx = Context::from_waker(&waker);

    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    assert!(timer.borrow_mut().poll_interrupt(&mut cpu_cx).is_pending());
    clock.advance_to(1_054_925_400);
    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    assert!(timer.borrow_mut().poll_interrupt(&mut cpu_cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);

    clock.advance_to(1_054_925_401);
    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert_eq!(
        timer.borrow_mut().poll_interrupt(&mut cpu_cx),
        Poll::Ready(0x08)
    );
    assert!(timer.borrow_mut().poll_interrupt(&mut cpu_cx).is_pending());

    // A replacement CPU waiter must receive subsequent edges.
    let replaced = wakes;
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut cpu_cx = Context::from_waker(&waker);
    assert!(timer.borrow_mut().poll_interrupt(&mut cpu_cx).is_pending());

    // A delayed host poll creates one pending request and keeps the original phase.
    clock.advance_to(1_000_000_000 + 10 * 54_925_401 + 1_234);
    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert_eq!(replaced.0.load(Ordering::Relaxed), 1);
    assert_eq!(
        timer.borrow_mut().poll_interrupt(&mut cpu_cx),
        Poll::Ready(0x08)
    );
    assert!(timer.borrow_mut().poll_interrupt(&mut cpu_cx).is_pending());
    clock.advance_to(1_000_000_000 + 11 * 54_925_401 - 1);
    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    assert!(timer.borrow_mut().poll_interrupt(&mut cpu_cx).is_pending());
    clock.advance_to(1_000_000_000 + 11 * 54_925_401);
    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::Relaxed), 2);
    assert_eq!(
        timer.borrow_mut().poll_interrupt(&mut cpu_cx),
        Poll::Ready(0x08)
    );

    // Edges also coalesce when the activity runs but the CPU does not acknowledge.
    clock.advance_to(1_000_000_000 + 12 * 54_925_401);
    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    clock.advance_to(1_000_000_000 + 13 * 54_925_401);
    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    assert_eq!(
        timer.borrow_mut().poll_interrupt(&mut cpu_cx),
        Poll::Ready(0x08)
    );
    assert!(timer.borrow_mut().poll_interrupt(&mut cpu_cx).is_pending());
}
