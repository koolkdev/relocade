//! INT 13h behavior for the PC's configuration without disk controllers.

use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, MemoryView,
};

const FLOPPY_STATUS: u32 = 0x441;
const HARD_DISK_STATUS: u32 = 0x474;
const INVALID_COMMAND: u8 = 0x01;
const PARAMETER_FAILURE: u8 = 0x07;

#[interrupt(context(call), args(function = ah))]
pub(crate) async fn dispatch(call: &mut CpuContext<'_>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x01 => last_status,
            0x08 => parameters,
            0x15 => drive_type,
            _ => unavailable,
        }
    );
}

fn status_address(drive: u8) -> u32 {
    if drive & 0x80 == 0 {
        FLOPPY_STATUS
    } else {
        HARD_DISK_STATUS
    }
}

fn save_status(memory: &mut MemoryView<'_>, drive: u8, status: u8) -> u8 {
    memory
        .write_u8(status_address(drive), status)
        .expect("BIOS disk status");
    status
}

#[interrupt(context(memory), args(drive = dl), returns {
    status => { ah = status, cf = status != 0 },
})]
fn last_status(memory: &MemoryView<'_>, drive: u8) -> u8 {
    memory
        .read_u8(status_address(drive))
        .expect("BIOS disk status")
}

#[interrupt(context(memory), args(drive = dl), returns {
    status => { ah = status, dl = 0, cf = true },
})]
fn parameters(memory: &mut MemoryView<'_>, drive: u8) -> u8 {
    save_status(memory, drive, PARAMETER_FAILURE)
}

#[interrupt(context(memory), args(drive = dl), returns {
    kind => { ah = kind, cf = false },
})]
fn drive_type(memory: &mut MemoryView<'_>, drive: u8) -> u8 {
    save_status(memory, drive, 0) // No such drive.
}

#[interrupt(context(memory), args(drive = dl), returns {
    status => { ah = status, cf = true },
})]
fn unavailable(memory: &mut MemoryView<'_>, drive: u8) -> u8 {
    // This also rejects EDD installation checks without returning their signature.
    save_status(memory, drive, INVALID_COMMAND)
}
