//! PSP copies of INT 22h (termination), 23h (Ctrl-Break) and 24h (critical error).
//! DOS saves these three consecutive far pointers at PSP:000A..0015 and restores
//! the live guest values on exit. See MS-DOS PROC.ASM's abort_inner.

use super::PSP_BYTES;
use relocade_machine::{FarPointer, MemoryView};
use std::ops::Range;

const PSP_VECTORS: Range<usize> = 0x0a..0x16;
const VECTOR_TABLE: FarPointer = FarPointer {
    segment: 0,
    offset: 0x22 * 4,
};

pub(super) fn save(memory: &MemoryView<'_>, psp: &mut [u8; PSP_BYTES]) {
    memory
        .read(VECTOR_TABLE.address(), &mut psp[PSP_VECTORS])
        .expect("process interrupt vectors");
}

pub(super) fn restore(memory: &mut MemoryView<'_>, psp_segment: u16) {
    let saved = FarPointer {
        segment: psp_segment,
        offset: PSP_VECTORS.start as u16,
    };
    let mut vectors = [0; PSP_VECTORS.end - PSP_VECTORS.start];
    memory
        .read(saved.address(), &mut vectors)
        .expect("PSP interrupt vectors");
    memory
        .write(VECTOR_TABLE.address(), &vectors)
        .expect("process interrupt vectors");
}
