//! Fractional evidence and rounding direction share four control bits, keeping
//! the operation within the same three-operand storage as scalar expressions.

pub(crate) const GUARD: u8 = 1;
pub(crate) const STICKY: u8 = 2;
pub(crate) const NEAREST: u8 = 4;
pub(crate) const AWAY: u8 = 8;

pub(super) fn evaluate(integer: u64, distance: u32, control: u8) -> [u64; 3] {
    let guard = control & GUARD != 0;
    let sticky = control & STICKY != 0;
    let (integer, guard, sticky) = match distance {
        0 => (integer, guard, sticky),
        1..=64 => (
            integer.checked_shr(distance).unwrap_or(0),
            integer & (1 << (distance - 1)) != 0,
            sticky || guard || integer & ((1 << (distance - 1)) - 1) != 0,
        ),
        _ => (0, false, sticky || guard || integer != 0),
    };
    let inexact = guard || sticky;
    let incremented = if control & NEAREST != 0 {
        guard && (sticky || integer & 1 != 0)
    } else {
        control & AWAY != 0 && inexact
    };
    [
        integer.wrapping_add(u64::from(incremented)),
        u64::from(inexact),
        u64::from(incremented),
    ]
}
