//! BIOS time-of-day data and the default timer interrupt source.

mod timer;
pub use timer::connect;

use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    MemoryView,
};

pub(crate) const CLOCK_TICKS: u32 = 0x46c;
const MIDNIGHT_ROLLOVER: u32 = 0x470;

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

interrupt_dispatch! {
    pub(crate) dispatch(ah) {
        0x00: read_clock,
        _: crate::interrupt::unsupported,
    }
}
