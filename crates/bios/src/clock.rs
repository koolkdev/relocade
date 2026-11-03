//! BIOS time-of-day data, advanced by IRQ0 and read through INT 1Ah.

mod timer;
pub use timer::connect;

use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, MemoryView,
};

const CLOCK_TICKS: u32 = 0x46c;
const MIDNIGHT_ROLLOVER: u32 = 0x470;
const TICKS_PER_DAY: u32 = 0x18_00b0;

pub(crate) fn initialize(memory: &mut MemoryView<'_>) {
    memory.write(CLOCK_TICKS, &[0; 5]).expect("BIOS clock data");
}

/// Advances time for one delivered IRQ0. Missed timer edges are not reconstructed.
/// The midnight byte counts rollovers until INT 1Ah consumes it.
pub(crate) fn tick(memory: &mut MemoryView<'_>) {
    let ticks = memory.read_u32(CLOCK_TICKS).expect("BIOS clock ticks");
    if ticks >= TICKS_PER_DAY - 1 {
        memory.write_u32(CLOCK_TICKS, 0).expect("BIOS clock ticks");
        let rollover = memory
            .read_u8(MIDNIGHT_ROLLOVER)
            .expect("BIOS midnight rollover");
        memory
            .write_u8(MIDNIGHT_ROLLOVER, rollover.wrapping_add(1))
            .expect("BIOS midnight rollover");
    } else {
        memory
            .write_u32(CLOCK_TICKS, ticks + 1)
            .expect("BIOS clock ticks");
    }
}

/// Time returned by INT 1Ah/AH=00h. Reading consumes only the rollover byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockReading {
    pub ticks: u32,
    pub midnight_rollover: u8,
}

/// Reads the BIOS tick count and clears its midnight rollover indication.
#[interrupt(context(memory), returns {
    reading => { (cx, dx) = reading.ticks, al = reading.midnight_rollover },
})]
pub fn read_clock(memory: &mut MemoryView<'_>) -> ClockReading {
    let mut data = [0; 5];
    memory
        .read(CLOCK_TICKS, &mut data)
        .expect("BIOS clock data");
    memory
        .write(MIDNIGHT_ROLLOVER, &[0])
        .expect("BIOS midnight rollover");
    ClockReading {
        ticks: u32::from_le_bytes(data[..4].try_into().unwrap()),
        midnight_rollover: data[4],
    }
}

#[interrupt(context(call), args(function = ah))]
pub(crate) async fn dispatch(call: &mut CpuContext<'_>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => read_clock,
            _ => crate::interrupt::unsupported,
        }
    );
}
