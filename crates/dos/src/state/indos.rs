//! DOS's resident reentrancy byte, returned by INT 21h/AH=34h.

use relocade_machine::{FarPointer, MemoryBus};

// Resident DOS workspace, above the BIOS data area and below the process arena.
// DOS 3.1+ reserves the preceding byte for its critical-error flag.
const RESIDENT_DATA: FarPointer = FarPointer {
    segment: 0x80,
    offset: 0,
};
pub(crate) const ADDRESS: FarPointer = RESIDENT_DATA.wrapping_add(1);

pub(crate) struct InDos(MemoryBus);

impl InDos {
    pub(crate) fn new(memory: MemoryBus) -> Self {
        memory
            .view()
            .write(RESIDENT_DATA.address(), &[0; 2])
            .unwrap();
        Self(memory)
    }

    pub(crate) fn enter(&self) -> Call<'_> {
        self.add(1);
        Call(self)
    }

    fn add(&self, delta: i8) {
        let mut memory = self.0.view();
        let count = memory.read_u8(ADDRESS.address()).unwrap();
        memory
            .write_u8(ADDRESS.address(), count.wrapping_add_signed(delta))
            .unwrap();
    }
}

/// A suspended service remains inside DOS. Dropping its run waiter does not
/// finish this scope; completing or abandoning the service itself does.
pub(crate) struct Call<'a>(&'a InDos);

impl Drop for Call<'_> {
    fn drop(&mut self) {
        self.0.add(-1);
    }
}
