//! AT keyboard and 8042 controller, connected to a master PIC IRQ line.

mod controller;
mod keyboard;
mod scancodes;

use controller::Controller;
use futures_util::future::{select, Either};
use relocade_frontend::keyboard::{self as frontend, KeyboardEvents, KeyboardInput};
use relocade_machine::{Clock, Device, DeviceConnection, IoBus, Runner};
use relocade_pic::IrqLine;
use std::{cell::RefCell, future::pending, ops::RangeInclusive, pin::pin, rc::Rc};

pub const DATA_PORT: u16 = 0x60;
pub const STATUS_COMMAND_PORT: u16 = 0x64;
pub const PORTS: [RangeInclusive<u16>; 2] = [
    DATA_PORT..=DATA_PORT,
    STATUS_COMMAND_PORT..=STATUS_COMMAND_PORT,
];
const OUTPUT_BUFFER_FULL: u32 = 1;

/// Consumes one controller output byte when available, through the guest bus.
pub fn read_scan_code(io: &IoBus) -> Option<u8> {
    (io.read_port(STATUS_COMMAND_PORT, 1) & OUTPUT_BUFFER_FULL != 0)
        .then(|| io.read_port(DATA_PORT, 1) as u8)
}

/// Registers an execution-owned keyboard and its event activity. Host text is
/// ignored: physical positions become scan bytes, with guest-programmed repeat.
pub async fn connect<C: Clock + Clone + 'static>(
    runner: &Runner<C>,
    irq: IrqLine,
) -> (KeyboardInput, DeviceConnection) {
    let (input, events) = frontend::channel();
    let device = Rc::new(RefCell::new(KeyboardController {
        clock: runner.clock().clone(),
        controller: Controller::new(irq),
        events: Some(events),
        changed: async_channel::bounded(1),
    }));
    (input, runner.connect(device, [], PORTS).await)
}

/// Port state and activity share one execution owner. Keyboard scanning and
/// controller register responsibilities are implemented in separate modules.
struct KeyboardController<C> {
    clock: C,
    controller: Controller,
    events: Option<KeyboardEvents>,
    changed: (async_channel::Sender<()>, async_channel::Receiver<()>),
}

impl<C: Clock + Clone> Device for KeyboardController<C> {
    async fn run(device: Rc<RefCell<Self>>) {
        let (clock, mut events, changed) = {
            let mut device = device.borrow_mut();
            (
                device.clock.clone(),
                device.events.take().expect("one keyboard activity"),
                device.changed.1.clone(),
            )
        };
        loop {
            let deadline = device.borrow().controller.keyboard.deadline;
            let timer = async {
                if let Some(deadline) = deadline {
                    clock.wait_until(deadline).await;
                } else {
                    pending::<()>().await;
                }
            };
            let event = match select(
                pin!(events.recv()),
                pin!(select(pin!(timer), pin!(changed.recv()))),
            )
            .await
            {
                Either::Left((event, _)) => Some(event),
                Either::Right(_) => None,
            };
            {
                let mut device = device.borrow_mut();
                match event {
                    Some(Ok(event)) => device.controller.keyboard.event(event, clock.now()),
                    Some(Err(_)) => {
                        device
                            .controller
                            .keyboard
                            .event(frontend::KeyboardEvent::FocusLost, clock.now());
                        device.controller.refill();
                        return;
                    }
                    None => device.controller.repeat(clock.now()),
                }
                device.controller.refill();
            }
            relocade_machine::yield_now().await;
        }
    }

    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        if bytes != 1 {
            return u32::MAX;
        }
        match port {
            DATA_PORT => self.controller.read_data().into(),
            STATUS_COMMAND_PORT => self.controller.status().into(),
            _ => u32::MAX,
        }
    }

    fn write_port(&mut self, port: u16, bytes: u8, value: u32) {
        if bytes != 1 {
            return;
        }
        match port {
            DATA_PORT => self.controller.data(value as u8),
            STATUS_COMMAND_PORT => self.controller.command(value as u8),
            _ => return,
        }
        let _ = self.changed.0.try_send(());
    }
}

#[cfg(test)]
mod tests;
