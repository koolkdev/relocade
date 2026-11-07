//! BIOS time-of-day data, advanced by IRQ0 and read through INT 1Ah.

use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    registers as reg, CpuContext, MemoryView,
};
use relocade_x86::{Segment, SegmentRegister};

const CLOCK_TICKS: u32 = 0x46c;
const MIDNIGHT_ROLLOVER: u32 = 0x470;
const TICKS_PER_DAY: u32 = 0x18_00b0;
const BIOS_DATA_SEGMENT: u16 = 0x40;

pub(crate) fn initialize(memory: &mut MemoryView<'_>) {
    memory.write(CLOCK_TICKS, &[0; 5]).expect("BIOS clock data");
}

/// Services IRQ0 using the AT BIOS register and callback convention.
pub(crate) async fn timer(call: &mut CpuContext<'_>) {
    call.cpu
        .with_saved_registers([reg::ax, reg::dx, reg::segment(Segment::Ds)], async {
            call.cpu.set_ax(BIOS_DATA_SEGMENT);
            call.cpu.set_segment(
                Segment::Ds,
                SegmentRegister::real_mode(Segment::Ds, BIOS_DATA_SEGMENT),
            );
            tick(&mut call.memory);
            call.call_interrupt(crate::interrupts::TIMER_TICK).await;
        })
        .await;
}

/// Advances time for one delivered IRQ0. Missed timer edges are not reconstructed.
/// The midnight byte counts rollovers until INT 1Ah consumes it.
fn tick(memory: &mut MemoryView<'_>) {
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
            _ => crate::interrupts::unsupported,
        }
    );
}
