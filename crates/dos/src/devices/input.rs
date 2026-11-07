//! Converts whole BIOS keystrokes into DOS input bytes.
//!
//! DOS character devices expose a byte stream. Ordinary keys produce only their
//! character byte; function and cursor keys produce 00h followed by the scan code.
//! For example, F1 is [00h, 3Bh]. A buffer read can return both bytes together.
//!
//! BIOS consumes the whole keystroke at once. This driver retains the encoded
//! byte iterator across reads and awaits another key only when it is exhausted.
//! Peeking observes pending bytes or the next untouched BIOS key.

use relocade_bios::{KeyMode, KeyStroke, Keyboard};
use std::{cell::RefCell, iter::Peekable, rc::Rc};

type DosBytes = std::iter::Flatten<std::array::IntoIter<Option<u8>, 2>>;
pub(crate) const CONTROL_C: u8 = 0x03;

/// A BIOS keystroke becomes one DOS character byte, or a zero/scan-code pair.
pub(crate) fn dos_bytes(key: KeyStroke) -> DosBytes {
    // Enhanced navigation keys carry E0h; a scan-zero OEM character does not.
    let special = matches!(key.character, 0 | 0xe0) && key.scan_code != 0;
    [
        Some(if special { 0 } else { key.character }),
        special.then_some(key.scan_code),
    ]
    .into_iter()
    .flatten()
}

/// Each console driver owns its byte conversion state. Open handles to that
/// driver share it; a separately installed driver starts with no pending byte.
#[derive(Clone)]
pub(crate) struct ConsoleInput(Rc<Input>);

struct Input {
    keyboard: Keyboard,
    pending_bytes: RefCell<Peekable<DosBytes>>,
}

impl ConsoleInput {
    pub(crate) fn new(keyboard: Keyboard) -> Self {
        Self(Rc::new(Input {
            keyboard,
            pending_bytes: RefCell::new([None; 2].into_iter().flatten().peekable()),
        }))
    }

    /// DOS's INT 1Bh handler replaces the driver's pending character with ^C.
    /// The BIOS's subsequent dummy key wakes a read already waiting in INT 16h.
    pub(crate) fn control_break(&self) {
        *self.0.pending_bytes.borrow_mut() =
            [Some(CONTROL_C), None].into_iter().flatten().peekable();
    }

    pub(super) fn peek_character(&self) -> Option<u8> {
        if let Some(&byte) = self.0.pending_bytes.borrow_mut().peek() {
            return Some(byte);
        }
        // Do not consume a BIOS key merely to populate the byte iterator.
        loop {
            let key = self.0.keyboard.peek_key(KeyMode::Enhanced)?;
            if key.character != 0 || key.scan_code != 0 {
                return dos_bytes(key).next();
            }
            self.0.keyboard.try_read_key(KeyMode::Enhanced);
        }
    }

    /// Clears this driver's pending bytes and the shared BIOS queue. Bytes
    /// already consumed by another driver remain with that driver.
    pub(super) fn flush_input(&self) {
        *self.0.pending_bytes.borrow_mut() = [None; 2].into_iter().flatten().peekable();
        self.0.keyboard.discard_queued();
    }

    pub(super) async fn read(&self, bytes: &mut [u8]) {
        for byte in bytes {
            *byte = self.read_dos_byte().await;
        }
    }

    async fn read_dos_byte(&self) -> u8 {
        loop {
            if let Some(byte) = self.0.pending_bytes.borrow_mut().next() {
                return byte;
            }
            // No iterator borrow spans the wait. Once BIOS supplies a key,
            // encoding it and returning its first byte complete without yielding.
            let key = self.0.keyboard.read_key(KeyMode::Enhanced).await;
            // 0000h is the BIOS break sentinel, not a DOS NUL character. Retry
            // the pending byte first: INT 1Bh may have placed ^C there meanwhile.
            if key.character != 0 || key.scan_code != 0 {
                *self.0.pending_bytes.borrow_mut() = dos_bytes(key).peekable();
            }
        }
    }
}
