//! The guest-visible keyboard flags and circular buffer in the BIOS data area.

use super::{KeyStroke, ShiftState};
use relocade_machine::{FarPointer, Memory, MemoryView};

// IBM PC/AT keyboard RAM, segment 0040h. Head/tail and bounds are 16-bit
// offsets within that segment, not linear addresses. The end is exclusive.
const BDA: FarPointer = FarPointer {
    segment: 0x0040,
    offset: 0,
};
const SHIFT_FLAGS: FarPointer = BDA.wrapping_add(0x17);
const HELD_KEYS: FarPointer = BDA.wrapping_add(0x18);
const HEAD: FarPointer = BDA.wrapping_add(0x1a);
const TAIL: FarPointer = BDA.wrapping_add(0x1c);
const BUFFER_START: FarPointer = BDA.wrapping_add(0x80);
const BUFFER_END: FarPointer = BDA.wrapping_add(0x82);
const KEYBOARD_STATUS: FarPointer = BDA.wrapping_add(0x96);

const DEFAULT_BUFFER_START: u16 = 0x1e;
const DEFAULT_BUFFER_END: u16 = 0x3e;
const KEYSTROKE_BYTES: u16 = 2; // Character byte followed by BIOS scan code.

pub(super) struct DataArea(Memory);

// These are three different BIOS bytes. In particular, a toggle being active
// (0040:0017) is distinct from the corresponding key being held (0040:0018).
bitflags::bitflags! {
    #[derive(Clone, Copy)]
    pub(super) struct ShiftFlags: u8 {
        const RIGHT_SHIFT = 1 << 0;
        const LEFT_SHIFT = 1 << 1;
        const CONTROL = 1 << 2;
        const ALT = 1 << 3;
        const SCROLL_LOCK = 1 << 4;
        const NUM_LOCK = 1 << 5;
        const CAPS_LOCK = 1 << 6;
        const INSERT = 1 << 7;
    }
    #[derive(Clone, Copy)]
    pub(super) struct HeldKeys: u8 {
        const LEFT_CONTROL = 1 << 0;
        const LEFT_ALT = 1 << 1;
        const SYS_REQUEST = 1 << 2;
        const PAUSED = 1 << 3;
        const SCROLL_LOCK = 1 << 4;
        const NUM_LOCK = 1 << 5;
        const CAPS_LOCK = 1 << 6;
        const INSERT = 1 << 7;
    }
    #[derive(Clone, Copy)]
    pub(super) struct KeyboardStatus: u8 {
        const PAUSE_PREFIX = 1 << 0;
        const EXTENDED_PREFIX = 1 << 1;
        const RIGHT_CONTROL = 1 << 2;
        const RIGHT_ALT = 1 << 3;
        const ENHANCED_KEYBOARD = 1 << 4;
    }
}

pub(super) struct Flags {
    pub shifts: ShiftState,
    pub held: HeldKeys,
    pub status: KeyboardStatus,
}

impl DataArea {
    pub fn new(memory: Memory) -> Self {
        {
            let mut view = memory.view();
            for (pointer, value) in [
                (HEAD, DEFAULT_BUFFER_START),
                (TAIL, DEFAULT_BUFFER_START),
                (BUFFER_START, DEFAULT_BUFFER_START),
                (BUFFER_END, DEFAULT_BUFFER_END),
            ] {
                view.write_u16(pointer.address(), value)
                    .expect("BIOS keyboard buffer");
            }
            view.write_u8(SHIFT_FLAGS.address(), 0)
                .expect("BIOS keyboard flags");
            view.write_u8(HELD_KEYS.address(), 0)
                .expect("BIOS keyboard flags");
            view.write_u8(
                KEYBOARD_STATUS.address(),
                KeyboardStatus::ENHANCED_KEYBOARD.bits(),
            )
            .expect("enhanced keyboard installed");
        }
        Self(memory)
    }

    fn advance(memory: &MemoryView<'_>, offset: u16) -> u16 {
        let next = offset.wrapping_add(KEYSTROKE_BYTES);
        if next
            == memory
                .read_u16(BUFFER_END.address())
                .expect("BIOS keyboard buffer end")
        {
            memory
                .read_u16(BUFFER_START.address())
                .expect("BIOS keyboard buffer start")
        } else {
            next
        }
    }

    /// IRQ input is dropped when full, as with the BIOS's fifteen-word buffer.
    pub fn push(&self, key: KeyStroke) -> bool {
        let mut memory = self.0.view();
        let tail = memory.read_u16(TAIL.address()).expect("BIOS keyboard tail");
        let next = Self::advance(&memory, tail);
        if next == memory.read_u16(HEAD.address()).expect("BIOS keyboard head") {
            return false;
        }
        memory
            .write(
                BDA.wrapping_add(usize::from(tail)).address(),
                &[key.character, key.scan_code],
            )
            .expect("BIOS keystroke");
        memory
            .write_u16(TAIL.address(), next)
            .expect("BIOS keyboard tail");
        true
    }

    pub fn peek(&self) -> Option<KeyStroke> {
        let memory = self.0.view();
        let head = memory.read_u16(HEAD.address()).expect("BIOS keyboard head");
        if head == memory.read_u16(TAIL.address()).expect("BIOS keyboard tail") {
            return None;
        }
        let word = memory
            .read_u16(BDA.wrapping_add(usize::from(head)).address())
            .expect("BIOS keystroke");
        let [character, scan_code] = word.to_le_bytes();
        Some(KeyStroke {
            character,
            scan_code,
        })
    }

    pub fn read(&self) -> Option<KeyStroke> {
        let key = self.peek()?;
        let mut memory = self.0.view();
        let head = memory.read_u16(HEAD.address()).expect("BIOS keyboard head");
        let next = Self::advance(&memory, head);
        memory
            .write_u16(HEAD.address(), next)
            .expect("BIOS keyboard head");
        Some(key)
    }

    pub fn discard(&self) {
        let mut memory = self.0.view();
        let tail = memory.read_u16(TAIL.address()).expect("BIOS keyboard tail");
        memory
            .write_u16(HEAD.address(), tail)
            .expect("BIOS keyboard head");
    }

    pub fn flags(&self) -> Flags {
        let memory = self.0.view();
        Flags {
            shifts: ShiftState::from_bits(
                memory
                    .read_u8(SHIFT_FLAGS.address())
                    .expect("BIOS shift flags"),
            ),
            held: HeldKeys::from_bits_retain(
                memory.read_u8(HELD_KEYS.address()).expect("BIOS held keys"),
            ),
            status: KeyboardStatus::from_bits_retain(
                memory
                    .read_u8(KEYBOARD_STATUS.address())
                    .expect("BIOS keyboard status"),
            ),
        }
    }

    pub fn set_flags(&self, flags: Flags) {
        let mut memory = self.0.view();
        memory
            .write_u8(SHIFT_FLAGS.address(), flags.shifts.bits())
            .expect("BIOS keyboard flags");
        memory
            .write_u8(HELD_KEYS.address(), flags.held.bits())
            .expect("BIOS keyboard pressed flags");
        memory
            .write_u8(KEYBOARD_STATUS.address(), flags.status.bits())
            .expect("BIOS enhanced keyboard flags");
    }

    pub fn shift_state(&self) -> ShiftState {
        self.flags().shifts
    }

    pub fn set_shift_state(&self, shifts: ShiftState) {
        let mut flags = self.flags();
        flags.shifts = shifts;
        // The host setter has no side information for Ctrl/Alt; use the left key.
        flags.held.set(HeldKeys::LEFT_CONTROL, shifts.control);
        flags.held.set(HeldKeys::LEFT_ALT, shifts.alt);
        flags
            .status
            .remove(KeyboardStatus::RIGHT_CONTROL | KeyboardStatus::RIGHT_ALT);
        self.set_flags(flags);
    }
}
