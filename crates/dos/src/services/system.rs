//! DOS version, resident data and interrupt-vector services.

use relocade_machine::{real_mode_interrupt::interrupt, FarPointer, MemoryView};

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
    memory
        .write_u32(u32::from(vector) * 4, destination.packed())
        .expect("interrupt vector table");
}

#[interrupt(context(memory), args(vector = al), returns {
    destination => { ptr(es, bx) = destination },
})]
pub(super) fn get_vector(memory: &MemoryView<'_>, vector: u8) -> FarPointer {
    FarPointer::from_packed(
        memory
            .read_u32(u32::from(vector) * 4)
            .expect("interrupt vector table"),
    )
}
