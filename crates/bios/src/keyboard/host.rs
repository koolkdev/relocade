//! Translate host keyboard events into the BIOS queue on its execution owner.

mod keys;
use keys::{control_character, scan_code};

use super::{KeyStroke, Keyboard, ShiftState};
use relocade_machine::{Clock, Device, DeviceConnection, Runner};
use relocade_peripherals::keyboard::{
    self, KeyCode, KeyboardEvent, KeyboardEvents, KeyboardInput, Modifiers,
};
use std::{cell::RefCell, rc::Rc};

pub(super) fn connect<C: Clock>(
    keyboard: Keyboard,
    runner: &Runner<C>,
) -> (KeyboardInput, DeviceConnection) {
    let (input, events) = keyboard::channel();
    let device = Rc::new(RefCell::new(HostKeyboard {
        keyboard,
        events: Some(events),
    }));
    (input, runner.connect(device, [], []))
}

struct HostKeyboard {
    keyboard: Keyboard,
    events: Option<KeyboardEvents>,
}

impl Device for HostKeyboard {
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
    modifiers: Modifiers,
    shifts: ShiftState,
    left_shift: bool,
    right_shift: bool,
}

impl Translation {
    fn apply(&mut self, keyboard: &Keyboard, event: KeyboardEvent) {
        use KeyCode::*;
        let key = match event {
            KeyboardEvent::Modifiers(modifiers) => {
                self.modifiers = modifiers;
                None
            }
            KeyboardEvent::FocusLost => {
                self.modifiers = Modifiers::default();
                self.left_shift = false;
                self.right_shift = false;
                None
            }
            KeyboardEvent::Key {
                code,
                text,
                pressed,
                repeat,
            } => {
                match code {
                    ShiftLeft => self.left_shift = pressed,
                    ShiftRight => self.right_shift = pressed,
                    CapsLock if pressed && !repeat => self.shifts.caps_lock ^= true,
                    NumLock if pressed && !repeat => self.shifts.num_lock ^= true,
                    ScrollLock if pressed && !repeat => self.shifts.scroll_lock ^= true,
                    Insert if pressed && !repeat => self.shifts.insert ^= true,
                    _ => {}
                }
                pressed.then_some((code, text))
            }
        };
        self.shifts.left_shift = self.modifiers.shift && (self.left_shift || !self.right_shift);
        self.shifts.right_shift = self.modifiers.shift && self.right_shift;
        self.shifts.control = self.modifiers.control;
        self.shifts.alt = self.modifiers.alt;
        keyboard.set_shift_state(self.shifts);
        let Some((code, text)) = key else {
            return;
        };
        let Some(scan_code) = scan_code(code) else {
            return;
        };
        if self.modifiers.super_key {
            return;
        }
        if self.modifiers.alt {
            keyboard.push(KeyStroke {
                character: 0,
                scan_code,
            });
            return;
        }
        if self.modifiers.control {
            if let Some(character) = control_character(code, text.as_deref()) {
                keyboard.push(KeyStroke {
                    character,
                    scan_code,
                });
                return;
            }
        }
        let character = match code {
            Enter | NumpadEnter => Some(b'\r'),
            Backspace => Some(8),
            Tab => Some(b'\t'),
            Escape => Some(27),
            _ => None,
        };
        if let Some(character) = character {
            keyboard.push(KeyStroke {
                character,
                scan_code,
            });
        } else if let Some(text) = text {
            // This initial PC adapter accepts ASCII. Layout-provided text is
            // authoritative; non-ASCII needs an explicit guest code-page policy.
            for character in text.chars().filter(char::is_ascii) {
                keyboard.push(KeyStroke {
                    character: character as u8,
                    scan_code,
                });
            }
        } else if matches!(
            code,
            ArrowLeft
                | ArrowRight
                | ArrowUp
                | ArrowDown
                | Home
                | End
                | PageUp
                | PageDown
                | Insert
                | Delete
                | F1
                | F2
                | F3
                | F4
                | F5
                | F6
                | F7
                | F8
                | F9
                | F10
                | F11
                | F12
                | Numpad0
                | Numpad1
                | Numpad2
                | Numpad3
                | Numpad4
                | Numpad5
                | Numpad6
                | Numpad7
                | Numpad8
                | Numpad9
                | NumpadDecimal
        ) {
            keyboard.push(KeyStroke {
                character: 0,
                scan_code,
            });
        }
    }
}

#[cfg(test)]
mod tests;
