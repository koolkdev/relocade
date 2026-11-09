//! DOS environment queries, interrupt vectors and unavailable raw-disk access.

use relocade_machine::{
    real_mode_interrupt::{interrupt, read_vector, write_vector},
    CpuStateAccess, FarPointer, MemoryView,
};

const INVALID_DISK_UNIT: u16 = 0x0201;

#[interrupt(context(cpu), returns { () => { ax = INVALID_DISK_UNIT } })]
pub(crate) fn absolute_disk_unavailable(cpu: &CpuStateAccess) {
    // This interface returns through RETF, leaving the saved FLAGS untouched.
    cpu.set_carry(true);
    cpu.set_interrupts_enabled(true);
}

#[interrupt(returns { address => { ptr(es, bx) = address } })]
pub(super) fn indos() -> FarPointer {
    crate::state::indos::ADDRESS
}

pub(super) struct Version {
    major: u8,
    minor: u8,
    vendor_or_flags: u16,
    serial: u16,
}

/// DOS 5.00, with no serial number or special version flags.
#[interrupt(args(request = al), returns {
    version => { al = version.major, ah = version.minor, bx = version.vendor_or_flags, cx = version.serial },
})]
pub(super) fn version(request: u8) -> Version {
    Version {
        major: 5,
        minor: 0,
        vendor_or_flags: if request == 1 { 0 } else { 0xff00 },
        serial: 0,
    }
}

#[interrupt(context(memory), args(vector = al, destination = ptr(ds, dx)))]
pub(super) fn set_vector(memory: &mut MemoryView<'_>, vector: u8, destination: FarPointer) {
    write_vector(memory, vector, destination);
}

#[interrupt(context(memory), args(vector = al), returns {
    destination => { ptr(es, bx) = destination },
})]
pub(super) fn get_vector(memory: &MemoryView<'_>, vector: u8) -> FarPointer {
    read_vector(memory, vector)
}
