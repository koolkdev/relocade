//! Translate keyboard input events into the BIOS queue on its execution owner.

use super::{Keyboard, PhysicalKeyboard};
use relocade_frontend::keyboard::{self, KeyboardEvents, KeyboardInput};
use relocade_machine::{Clock, Device, DeviceConnection, Runner};
use std::{cell::RefCell, rc::Rc};

pub(super) async fn connect<C: Clock>(
    keyboard: Keyboard,
    runner: &Runner<C>,
) -> (KeyboardInput, DeviceConnection) {
    let (input, events) = keyboard::channel();
    let device = Rc::new(RefCell::new(InputAdapter {
        keyboard,
        events: Some(events),
    }));
    (input, runner.connect(device, [], []).await)
}

struct InputAdapter {
    keyboard: Keyboard,
    events: Option<KeyboardEvents>,
}

impl Device for InputAdapter {
    async fn run(device: Rc<RefCell<Self>>) {
        let (keyboard, mut events) = {
            let mut device = device.borrow_mut();
            (
                device.keyboard.clone(),
                device.events.take().expect("one keyboard activity"),
            )
        };
        let mut translation = PhysicalKeyboard::default();
        while let Ok(event) = events.recv().await {
            let key = translation.translate(event);
            keyboard.set_shift_state(translation.shift_state());
            if let Some(key) = key {
                let _ = keyboard.store_key(key); // Physical input is dropped when full.
            }
        }
    }
}
