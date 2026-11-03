//! Immediate adapter controls, using the same I/O ports as guest software.

use super::{read_indexed, write_indexed};
use relocade_machine::{real_mode_interrupt::interrupt, IoBus};
use relocade_vga::registers::{sequencer, ADAPTER_CONTROL_PORT, SEQUENCER_INDEX_PORT};

/// Enables CPU video addressing with selection 0, or disables it with 1.
/// Other selectors return false without accessing the device. Output continues.
#[interrupt(context(io), args(selection = al),
    returns { true => { al = 0x12 }, false => { al = 0 } }
)]
pub fn set_video_addressing(io: &IoBus, selection: u8) -> bool {
    // IBM's PC/AT VGA ROM writes these values for AH=12h/BL=32h.
    let control = match selection {
        0 => 0x0e,
        1 => 0,
        _ => return false,
    };
    io.write_port(ADAPTER_CONTROL_PORT, 1, control);
    true
}

/// Blanks or restores VGA output. Other selectors leave it unchanged.
/// The interrupt acknowledges every request, including invalid selectors.
#[interrupt(context(io), args(selection = al), returns { () => { al = 0x12 } })]
pub fn set_screen_output(io: &IoBus, selection: u8) {
    if selection <= 1 {
        let clocking = read_indexed(io, SEQUENCER_INDEX_PORT, sequencer::CLOCKING_MODE);
        let screen_off = if selection == 1 {
            sequencer::SCREEN_OFF
        } else {
            0
        };
        write_indexed(
            io,
            SEQUENCER_INDEX_PORT,
            sequencer::CLOCKING_MODE,
            (clocking & !sequencer::SCREEN_OFF) | screen_off,
        );
    }
}
