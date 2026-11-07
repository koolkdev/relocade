use super::super::*;
use futures_util::future::LocalBoxFuture;
use relocade_frontend::keyboard::KeyboardEvent;
use std::{
    cell::Cell,
    collections::BTreeMap,
    future::poll_fn,
    sync::{
        atomic::{AtomicBool, Ordering},
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
    next_waiter: Cell<usize>,
    waiters: RefCell<BTreeMap<usize, (Duration, Waker)>>,
}

struct Waiter(Rc<Time>, usize);

impl Drop for Waiter {
    fn drop(&mut self) {
        self.0.waiters.borrow_mut().remove(&self.1);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        self.0.now.get()
    }

    async fn wait_until(&self, deadline: Duration) {
        let id = self.0.next_waiter.get();
        self.0.next_waiter.set(id + 1);
        let _waiter = Waiter(self.0.clone(), id);
        poll_fn(|cx| {
            if self.now() >= deadline {
                Poll::Ready(())
            } else {
                self.0
                    .waiters
                    .borrow_mut()
                    .insert(id, (deadline, cx.waker().clone()));
                Poll::Pending
            }
        })
        .await;
    }
}

#[derive(Default)]
struct Notified(AtomicBool);

impl Wake for Notified {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub(super) struct Fixture {
    device: Rc<RefCell<KeyboardController<ManualClock>>>,
    activity: LocalBoxFuture<'static, ()>,
    clock: ManualClock,
    notified: Arc<Notified>,
    input: Option<KeyboardInput>,
}

impl Fixture {
    pub fn new(pic: &Pic) -> Self {
        let clock = ManualClock::default();
        let (input, events) = frontend::channel();
        let device = Rc::new(RefCell::new(KeyboardController {
            controller: Rc::new(Controller::new(pic)),
            startup: Some((clock.clone(), events)),
        }));
        let mut fixture = Self {
            activity: Box::pin(KeyboardController::run(device.clone())),
            device,
            clock,
            notified: Arc::default(),
            input: Some(input),
        };
        fixture.settle();
        fixture
    }

    /// Drive the real device activity until all notified work parks. No host
    /// timers or sleeps determine protocol assertions.
    pub fn settle(&mut self) {
        for _ in 0..256 {
            self.poll_activity();
            if !self.notified.0.load(Ordering::Relaxed) {
                return;
            }
        }
        panic!("keyboard activity did not park");
    }

    pub fn poll_activity(&mut self) {
        let waker = Waker::from(self.notified.clone());
        let mut cx = Context::from_waker(&waker);
        self.notified.0.store(false, Ordering::Relaxed);
        assert!(self.activity.as_mut().poll(&mut cx).is_pending());
    }

    pub fn status(&self) -> u8 {
        self.device.borrow_mut().read_port(STATUS_COMMAND_PORT, 1) as u8
    }

    pub fn write_unpolled(&mut self, port: u16, value: u8) {
        self.device
            .borrow_mut()
            .write_port(port, 1, u32::from(value));
    }

    fn write(&mut self, port: u16, value: u8) {
        assert_eq!(self.status() & INPUT_BUFFER_FULL, 0);
        self.write_unpolled(port, value);
        assert_ne!(self.status() & INPUT_BUFFER_FULL, 0);
        self.settle();
        self.finish_transfers();
    }

    pub fn command(&mut self, value: u8) {
        self.write(STATUS_COMMAND_PORT, value);
    }
    pub fn data(&mut self, value: u8) {
        self.write(DATA_PORT, value);
    }

    pub fn read_unpolled(&mut self) -> u8 {
        self.device.borrow_mut().read_port(DATA_PORT, 1) as u8
    }

    pub fn read_data(&mut self) -> u8 {
        let byte = self.read_unpolled();
        self.settle();
        self.finish_transfers();
        byte
    }

    pub fn event(&mut self, event: KeyboardEvent) {
        self.queue_event(event);
        self.settle();
        self.finish_transfers();
    }

    pub fn queue_event(&self, event: KeyboardEvent) {
        self.input.as_ref().unwrap().unbounded_send(event).unwrap();
    }

    pub fn disconnect(&mut self) {
        self.input.take();
        self.settle();
        self.finish_transfers();
    }

    pub fn advance(&mut self, now: Duration) {
        assert!(now >= self.clock.now());
        self.clock.0.now.set(now);
        let waiters: Vec<_> = self
            .clock
            .0
            .waiters
            .borrow()
            .values()
            .map(|(_, waker)| waker.clone())
            .collect();
        for waiter in waiters {
            waiter.wake();
        }
        self.settle();
    }

    pub fn now(&self) -> Duration {
        self.clock.now()
    }

    // Protocol tests can finish short wire transfers while leaving typematic
    // deadlines under their explicit control. Timing tests use advance directly.
    pub fn finish_transfers(&mut self) {
        let limit = self.now() + Duration::from_millis(10);
        for _ in 0..64 {
            let deadline = self
                .clock
                .0
                .waiters
                .borrow()
                .values()
                .map(|(deadline, _)| *deadline)
                .min();
            let Some(deadline) = deadline.filter(|deadline| *deadline <= limit) else {
                return;
            };
            self.advance(deadline);
        }
        panic!("keyboard short transfers did not finish");
    }
}
