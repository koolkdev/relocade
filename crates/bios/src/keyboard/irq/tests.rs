use super::*;
#[path = "../../../../machine/tests/support/clock.rs"]
mod clock;
use relocade_at_keyboard::{DATA_PORT, PORTS};
use relocade_machine::{Device, DeviceConnection};
use std::{
    cell::RefCell,
    rc::Rc,
    task::{Context, Poll, Waker},
};

#[derive(Debug, PartialEq)]
enum Access {
    Command(u8),
    Data,
    Complete,
}

#[derive(Default)]
struct Controller {
    busy: bool,
    stall: bool,
    accesses: Vec<Access>,
}

impl Device for Controller {
    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        assert_eq!(bytes, 1);
        match port {
            STATUS_COMMAND_PORT => u32::from(self.busy) * u32::from(INPUT_BUFFER_FULL),
            DATA_PORT => {
                self.accesses.push(Access::Data);
                0x1e
            }
            _ => panic!("unexpected port"),
        }
    }

    fn write_port(&mut self, port: u16, bytes: u8, value: u32) {
        assert_eq!((port, bytes), (STATUS_COMMAND_PORT, 1));
        self.accesses.push(Access::Command(value as u8));
        self.busy = self.stall;
    }
}

fn bus(controller: &Rc<RefCell<Controller>>) -> IoBus {
    IoBus::new([DeviceConnection {
        device: controller.clone(),
        mmio: vec![],
        ports: PORTS.into_iter().collect(),
    }])
}

#[test]
fn inhibition_encloses_the_latch_read_and_completion_even_for_an_error_result() {
    let controller = Rc::new(RefCell::new(Controller::default()));
    let io = bus(&controller);
    let clock = clock::ManualClock::default();
    let mut operation = pin!(with_keyboard_inhibited(&io, &clock, async {
        assert_eq!(io.read_port(DATA_PORT, 1), 0x1e);
        controller.borrow_mut().accesses.push(Access::Complete);
        Err::<(), _>("handler result")
    }));
    assert_eq!(
        operation
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Err("handler result"))
    );
    assert_eq!(
        controller.borrow().accesses,
        [
            Access::Command(DISABLE_INTERFACE),
            Access::Data,
            Access::Complete,
            Access::Command(ENABLE_INTERFACE),
        ]
    );
}

#[test]
fn stalled_readiness_or_acceptance_still_allows_the_read_and_enable_attempt() {
    for busy in [false, true] {
        let controller = Rc::new(RefCell::new(Controller {
            busy,
            stall: true,
            ..Controller::default()
        }));
        let io = bus(&controller);
        let clock = clock::ManualClock::default();
        let mut operation = pin!(with_keyboard_inhibited(&io, &clock, async {
            let byte = io.read_port(DATA_PORT, 1);
            controller.borrow_mut().accesses.push(Access::Complete);
            byte
        }));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(operation.as_mut().poll(&mut cx).is_pending());
        clock.advance(Duration::from_millis(499));
        assert!(operation.as_mut().poll(&mut cx).is_pending());
        assert!(!controller.borrow().accesses.contains(&Access::Data));
        clock.advance(Duration::from_millis(500));
        assert!(operation.as_mut().poll(&mut cx).is_pending());
        assert_eq!(
            controller.borrow().accesses,
            [
                Access::Command(DISABLE_INTERFACE),
                Access::Data,
                Access::Complete,
            ]
        );
        clock.advance(Duration::from_millis(1000));
        assert_eq!(operation.as_mut().poll(&mut cx), Poll::Ready(0x1e));
        assert_eq!(
            controller.borrow().accesses.last(),
            Some(&Access::Command(ENABLE_INTERFACE))
        );
    }
}

#[test]
fn abandoning_the_body_does_not_complete_the_outer_keyboard_irq() {
    let controller = Rc::new(RefCell::new(Controller::default()));
    let io = bus(&controller);
    let clock = clock::ManualClock::default();
    {
        let mut operation = pin!(with_keyboard_inhibited(
            &io,
            &clock,
            std::future::pending::<()>(),
        ));
        assert!(operation
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
    }
    assert_eq!(
        controller.borrow().accesses,
        [Access::Command(DISABLE_INTERFACE)]
    );
}
