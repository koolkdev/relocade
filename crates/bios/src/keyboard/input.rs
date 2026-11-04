//! Translate keyboard input events into the BIOS queue on its execution owner.

mod keys;
use keys::keystroke;

use super::{Keyboard, ShiftState};
use relocade_frontend::keyboard::{
    self, ElementState, KeyCode, KeyboardEvent, KeyboardEvents, KeyboardInput, PhysicalKey,
};
use relocade_machine::{Clock, Device, DeviceConnection, Runner};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

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
        let mut translation = Translation::default();
        while let Ok(event) = events.recv().await {
            translation.apply(&keyboard, event);
            relocade_machine::yield_now().await;
        }
    }
}

#[derive(Default)]
struct Translation {
    shifts: ShiftState,
    held: BTreeSet<KeyCode>,
}

impl Translation {
    fn apply(&mut self, keyboard: &Keyboard, event: KeyboardEvent) {
        use KeyCode::*;
        let key = match event {
            KeyboardEvent::Modifiers(modifiers) => {
                // Attachment and focus regain may supply a snapshot without key
                // presses. Preserve known sides; use the left key when unknown.
                for (active, left, right) in [
                    (modifiers.shift_key(), ShiftLeft, ShiftRight),
                    (modifiers.control_key(), ControlLeft, ControlRight),
                    (modifiers.alt_key(), AltLeft, AltRight),
                ] {
                    if !active {
                        self.held.remove(&left);
                        self.held.remove(&right);
                    } else if !self.held.contains(&left) && !self.held.contains(&right) {
                        self.held.insert(left);
                    }
                }
                None
            }
            KeyboardEvent::FocusLost => {
                self.held.clear();
                None
            }
            KeyboardEvent::Key {
                physical_key,
                state,
                ..
            } => {
                let PhysicalKey::Code(code) = physical_key else {
                    return;
                };
                let pressed = state == ElementState::Pressed;
                let fresh = if pressed {
                    self.held.insert(code)
                } else {
                    self.held.remove(&code);
                    false
                };
                match code {
                    CapsLock if fresh => self.shifts.caps_lock ^= true,
                    NumLock if fresh => self.shifts.num_lock ^= true,
                    ScrollLock if fresh => self.shifts.scroll_lock ^= true,
                    Insert if fresh => self.shifts.insert ^= true,
                    _ => {}
                }
                pressed.then_some(code)
            }
        };
        self.shifts.left_shift = self.held.contains(&ShiftLeft);
        self.shifts.right_shift = self.held.contains(&ShiftRight);
        self.shifts.control = self.held.contains(&ControlLeft) || self.held.contains(&ControlRight);
        self.shifts.alt = self.held.contains(&AltLeft) || self.held.contains(&AltRight);
        keyboard.set_shift_state(self.shifts);
        if let Some(key) = key.and_then(|code| keystroke(code, self.shifts)) {
            keyboard.push(key);
        }
    }
}

#[cfg(test)]
mod tests;
