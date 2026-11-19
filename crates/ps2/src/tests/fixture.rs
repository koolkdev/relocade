use super::super::*;
use relocade_clock_test_support::ManualClock;
use std::time::Duration;

pub(super) struct Fixture {
    io: IoBus,
    controller: Rc<Controller>,
    clock: ManualClock,
}

impl Fixture {
    pub fn new(pic: &Pic) -> Self {
        let clock = ManualClock::default();
        let controller = Rc::new(Controller::new(
            clock.clock(),
            pic,
            Some(mouse::Mouse::new(clock.clock())),
            None,
        ));
        let io = IoBus::default();
        controller.connect_ports(&io);
        Self {
            io,
            controller,
            clock,
        }
    }

    pub fn settle(&mut self) {
        while self.controller.due() {
            self.controller.service();
        }
    }

    pub fn status(&self) -> u8 {
        self.io.read_port(STATUS_COMMAND_PORT, 1) as u8
    }

    pub fn write_unpolled(&mut self, port: u16, value: u8) {
        self.io.write_port(port, 1, u32::from(value));
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
        self.io.read_port(DATA_PORT, 1) as u8
    }

    pub fn read_data(&mut self) -> u8 {
        let byte = self.read_unpolled();
        self.settle();
        byte
    }

    pub fn event(&mut self, event: KeyboardEvent) {
        self.controller.key_event(event);
    }

    pub fn mouse_event(&mut self, event: MouseEvent) {
        self.controller.mouse_event(event);
    }

    pub fn mouse_command(&mut self, value: u8) {
        self.command(0xd4);
        self.data(value);
    }

    pub fn advance(&mut self, now: Duration) {
        self.clock.advance_to(self.clock.at(now));
        self.settle();
    }
}
