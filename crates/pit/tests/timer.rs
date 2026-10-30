use relocade_machine::{Clock, IoBus, Runner};
use relocade_pit::Pit;
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
    fn advance(&self, nanoseconds: u64) {
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
    let runner = Runner::with_clock(clock.clone());
    let timer = Rc::new(RefCell::new(Pit::with_clock(Rc::new(clock.clone()))));
    let io = IoBus::new([runner.connect(timer, [], [])]);
    let mut devices = pin!(runner.run(async |_| std::future::pending::<()>().await));
    let mut device_cx = Context::from_waker(Waker::noop());
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut cpu_cx = Context::from_waker(&waker);

    assert!(devices.as_mut().poll(&mut device_cx).is_pending());
    assert!(io.poll_interrupt(&mut cpu_cx).is_pending());
    clock.advance(54_925_400);
    assert!(devices.as_mut().poll(&mut device_cx).is_pending());
    assert!(io.poll_interrupt(&mut cpu_cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);

    clock.advance(54_925_401);
    assert!(devices.as_mut().poll(&mut device_cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert_eq!(io.poll_interrupt(&mut cpu_cx), Poll::Ready(0x08));
    assert!(io.poll_interrupt(&mut cpu_cx).is_pending());

    // A delayed host poll creates one pending request and keeps the original phase.
    clock.advance(10 * 54_925_401 + 1_234);
    assert!(devices.as_mut().poll(&mut device_cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::Relaxed), 2);
    assert_eq!(io.poll_interrupt(&mut cpu_cx), Poll::Ready(0x08));
    assert!(io.poll_interrupt(&mut cpu_cx).is_pending());
    clock.advance(11 * 54_925_401 - 1);
    assert!(devices.as_mut().poll(&mut device_cx).is_pending());
    assert!(io.poll_interrupt(&mut cpu_cx).is_pending());
    clock.advance(11 * 54_925_401);
    assert!(devices.as_mut().poll(&mut device_cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::Relaxed), 3);
    assert_eq!(io.poll_interrupt(&mut cpu_cx), Poll::Ready(0x08));

    // Edges also coalesce when the activity runs but the CPU does not acknowledge.
    clock.advance(12 * 54_925_401);
    assert!(devices.as_mut().poll(&mut device_cx).is_pending());
    clock.advance(13 * 54_925_401);
    assert!(devices.as_mut().poll(&mut device_cx).is_pending());
    assert_eq!(io.poll_interrupt(&mut cpu_cx), Poll::Ready(0x08));
    assert!(io.poll_interrupt(&mut cpu_cx).is_pending());
}
