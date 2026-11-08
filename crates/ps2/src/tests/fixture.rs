use super::super::*;
use futures_util::future::LocalBoxFuture;
use relocade_clock_test_support::{origin as clock_origin, ManualClock};
use relocade_frontend::keyboard::KeyboardEvent;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Wake, Waker},
    time::Duration,
};

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
    device: Rc<RefCell<Ps2Controller>>,
    activity: LocalBoxFuture<'static, ()>,
    clock: ManualClock,
    notified: Arc<Notified>,
    input: Option<KeyboardInput>,
    mouse_input: Option<MouseInput>,
}

impl Fixture {
    pub fn new(pic: &Pic) -> Self {
        let clock = ManualClock::default();
        let (input, events) = frontend::channel();
        let (mouse_input, mouse_events) = relocade_frontend::mouse::channel();
        let device = Rc::new(RefCell::new(Ps2Controller {
            controller: Rc::new(Controller::new(pic, [Port::Keyboard, Port::Auxiliary])),
            startup: Some((clock.clock(), events, mouse_events)),
        }));
        let mut fixture = Self {
            activity: Box::pin(Ps2Controller::run(device.clone())),
            device,
            clock,
            notified: Arc::default(),
            input: Some(input),
            mouse_input: Some(mouse_input),
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
        byte
    }

    pub fn event(&mut self, event: KeyboardEvent) {
        self.queue_event(event);
        self.settle();
    }

    pub fn queue_event(&self, event: KeyboardEvent) {
        self.input.as_ref().unwrap().unbounded_send(event).unwrap();
    }

    pub fn disconnect(&mut self) {
        self.input.take();
        self.settle();
    }

    pub fn mouse_event(&mut self, event: relocade_frontend::mouse::MouseEvent) {
        self.mouse_input
            .as_ref()
            .unwrap()
            .unbounded_send(event)
            .unwrap();
        self.settle();
    }

    pub fn disconnect_mouse(&mut self) {
        self.mouse_input.take();
        self.settle();
    }

    pub fn mouse_command(&mut self, value: u8) {
        self.command(0xd4);
        self.data(value);
    }

    pub fn advance(&mut self, now: Duration) {
        self.clock.advance_to(clock_origin() + now);
        self.settle();
    }
}
