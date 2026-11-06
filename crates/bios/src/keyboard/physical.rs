//! Physical input for native applications, separate from the BIOS input queue.

use super::{layout, KeyStroke, ShiftState};
use relocade_frontend::keyboard::{ElementState, KeyCode, KeyboardEvent, PhysicalKey};
use std::collections::BTreeSet;

/// Translates physical events with the BIOS US layout, without touching guest RAM.
/// A native input reader owns one translator across successive lines. Host text
/// and composition do not select characters; host repeats produce repeated keys.
#[derive(Default)]
pub struct PhysicalKeyboard {
    shifts: ShiftState,
    held: BTreeSet<KeyCode>,
}

impl PhysicalKeyboard {
    pub fn shift_state(&self) -> ShiftState {
        self.shifts
    }

    /// Updates held keys and locks, returning a translated key on a key press.
    /// Focus loss releases held modifiers while preserving the lock state.
    pub fn translate(&mut self, event: KeyboardEvent) -> Option<KeyStroke> {
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
                    return None;
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
        key.and_then(|code| layout::keystroke(code, self.shifts))
    }
}

#[cfg(test)]
mod tests;
