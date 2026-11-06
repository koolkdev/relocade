//! Decode controller set-1 bytes into keys and update BIOS modifier state.

use super::{
    data::{DataArea, HeldKeys, KeyboardStatus},
    layout::{self, Key},
};

const EXTENDED_PREFIX: u8 = 0xe0;
const PAUSE_PREFIX: u8 = 0xe1;
const BREAK_BIT: u8 = 0x80;
const PAUSE_SUFFIX: [u8; 5] = [0x1d, 0x45, 0xe1, 0x9d, 0xc5];

#[derive(Default)]
pub(super) struct Decoder {
    pause_remaining: usize,
}

impl Decoder {
    pub fn scan(&mut self, data: &DataArea, byte: u8) -> bool {
        let mut flags = data.flags();
        if self.pause_remaining != 0 {
            self.pause_remaining -= 1;
            if self.pause_remaining == 0 {
                flags.status.remove(KeyboardStatus::PAUSE_PREFIX);
                data.set_flags(flags);
            }
            return false;
        }
        match byte {
            PAUSE_PREFIX => {
                self.pause_remaining = PAUSE_SUFFIX.len();
                flags.status.remove(KeyboardStatus::EXTENDED_PREFIX);
                flags.status.insert(KeyboardStatus::PAUSE_PREFIX);
                data.set_flags(flags);
                return false;
            }
            EXTENDED_PREFIX => {
                flags.status.insert(KeyboardStatus::EXTENDED_PREFIX);
                data.set_flags(flags);
                return false;
            }
            _ => {}
        }
        let extended = flags.status.contains(KeyboardStatus::EXTENDED_PREFIX);
        flags.status.remove(KeyboardStatus::EXTENDED_PREFIX);
        let pressed = byte & BREAK_BIT == 0;
        let Some(key) = Key::from_scan(byte & !BREAK_BIT, extended) else {
            // Includes fake shifts, Print Screen, Break, GUI keys and device replies.
            data.set_flags(flags);
            return false;
        };
        match key {
            Key::LeftShift => flags.shifts.left_shift = pressed,
            Key::RightShift => flags.shifts.right_shift = pressed,
            Key::LeftControl => flags.held.set(HeldKeys::LEFT_CONTROL, pressed),
            Key::RightControl => flags.status.set(KeyboardStatus::RIGHT_CONTROL, pressed),
            Key::LeftAlt => flags.held.set(HeldKeys::LEFT_ALT, pressed),
            Key::RightAlt => flags.status.set(KeyboardStatus::RIGHT_ALT, pressed),
            Key::CapsLock => toggle(
                &mut flags.shifts.caps_lock,
                &mut flags.held,
                HeldKeys::CAPS_LOCK,
                pressed,
            ),
            Key::NumLock => toggle(
                &mut flags.shifts.num_lock,
                &mut flags.held,
                HeldKeys::NUM_LOCK,
                pressed,
            ),
            Key::ScrollLock => toggle(
                &mut flags.shifts.scroll_lock,
                &mut flags.held,
                HeldKeys::SCROLL_LOCK,
                pressed,
            ),
            Key::Insert | Key::Keypad0 => {
                let numeric = key == Key::Keypad0 && flags.shifts.numeric_keypad();
                if numeric {
                    flags.held.set(HeldKeys::INSERT, pressed);
                } else {
                    toggle(
                        &mut flags.shifts.insert,
                        &mut flags.held,
                        HeldKeys::INSERT,
                        pressed,
                    );
                }
            }
            _ => {}
        }
        // Either side keeps Ctrl/Alt active; releasing the other side cannot clear it.
        if matches!(key, Key::LeftControl | Key::RightControl) {
            flags.shifts.control = flags.held.contains(HeldKeys::LEFT_CONTROL)
                || flags.status.contains(KeyboardStatus::RIGHT_CONTROL);
        }
        if matches!(key, Key::LeftAlt | Key::RightAlt) {
            flags.shifts.alt = flags.held.contains(HeldKeys::LEFT_ALT)
                || flags.status.contains(KeyboardStatus::RIGHT_ALT);
        }
        let shifts = flags.shifts;
        data.set_flags(flags);
        pressed && layout::keystroke(key, shifts).is_some_and(|key| data.push(key))
    }
}

fn toggle(state: &mut bool, held: &mut HeldKeys, key: HeldKeys, pressed: bool) {
    if pressed && !held.contains(key) {
        *state = !*state;
    }
    held.set(key, pressed);
}
