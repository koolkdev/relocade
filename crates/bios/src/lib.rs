//! BIOS services for a real-mode PC, reached through ordinary runtime hooks.
//!
//! Initially implements INT 1Ah/AH=00h: read ticks and midnight rollover from
//! the BIOS data area at 0040:006C and 0040:0070. Initialization sets both to zero.
//! A machine timer must update these fields for time to advance.
#![forbid(unsafe_code)]

use relocade_machine::Memory;
use relocade_runtime::{real_mode_interrupt, Hook, HostCall};
use std::ops::ControlFlow;

const BIOS_SEGMENT: u16 = 0xf000;
const CLOCK_ENTRY: u16 = 0xff00;
const CLOCK_ADDRESS: u32 = ((BIOS_SEGMENT as u32) << 4) + CLOCK_ENTRY as u32;
const CLOCK_TICKS: u32 = 0x46c;
const MIDNIGHT_ROLLOVER: u32 = 0x470;

/// Initializes the clock data, INT 1Ah vector and its ROM return instruction.
/// The machine must map low memory as RAM and F0000..FFFFF as identity-backed ROM.
pub fn initialize(memory: &mut Memory<'_>) {
    memory.write(CLOCK_TICKS, &[0; 5]).expect("BIOS clock data");
    let [offset_lo, offset_hi] = CLOCK_ENTRY.to_le_bytes();
    let [segment_lo, segment_hi] = BIOS_SEGMENT.to_le_bytes();
    memory
        .write(0x1a * 4, &[offset_lo, offset_hi, segment_lo, segment_hi])
        .expect("BIOS interrupt vector");
    memory
        .write_backing(CLOCK_ADDRESS, &real_mode_interrupt::HOOK_STUB)
        .expect("BIOS ROM backing");
}

/// Returns hooks for installation alongside another environment's hooks.
/// BIOS handlers do not access the environment's state.
pub fn hooks<T: 'static>() -> [Hook<T>; 1] {
    [Hook::new(CLOCK_ADDRESS, clock)]
}

fn clock<T>(call: &mut HostCall<'_, T>) -> ControlFlow<()> {
    if call.cpu().ah() == 0 {
        let mut data = [0; 5];
        call.memory()
            .read(CLOCK_TICKS, &mut data)
            .expect("BIOS clock data");
        let mut cpu = call.cpu();
        cpu.set_dx(u16::from_le_bytes([data[0], data[1]]));
        cpu.set_cx(u16::from_le_bytes([data[2], data[3]]));
        cpu.set_al(data[4]);
        call.memory()
            .write(MIDNIGHT_ROLLOVER, &[0])
            .expect("BIOS midnight rollover");
    } else {
        // Unsupported BIOS function.
        call.cpu().set_ah(0x86);
        real_mode_interrupt::set_carry(call, true);
    }
    real_mode_interrupt::return_from_interrupt(call)
}
