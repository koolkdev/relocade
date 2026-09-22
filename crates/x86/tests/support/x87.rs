//! Literal architectural status words keep x87 behavior fixtures readable.

use wasm86_x86::StoredX87Status;

pub(crate) const fn status(word: u16) -> StoredX87Status {
    StoredX87Status {
        exception_flags: (word & 0x007f) as u8,
        top: ((word >> 11) & 7) as u8,
        c0: ((word >> 8) & 1) as u8,
        c1: ((word >> 9) & 1) as u8,
        c2: ((word >> 10) & 1) as u8,
        c3: ((word >> 14) & 1) as u8,
        error_summary: ((word >> 7) & 1) as u8,
        busy: ((word >> 15) & 1) as u8,
    }
}
