//! INT 10h string formats and cursor-update policy.
use super::unsupported;
use crate::screen::Output;
use relocade_machine::{real_mode_interrupt, HookResult};

pub(super) fn interrupt(call: &mut crate::BiosContext<'_>) -> HookResult {
    let format = call.cpu.al();
    if format > 3 {
        return unsupported(call);
    }
    let count = call.cpu.cx();
    if count == 0 {
        return real_mode_interrupt::return_from_interrupt(call.cpu);
    }
    let Some(mut output) = Output::new(&mut call.memory, call.io, call.cpu.bh()) else {
        return unsupported(call);
    };
    output.set_position(call.cpu.dx());
    output.write_string(
        call.cpu.segments().es.base,
        call.cpu.bp(),
        count,
        (format & 2 == 0).then_some(call.cpu.bl()),
    );
    if format & 1 != 0 {
        output.commit_cursor();
    }
    real_mode_interrupt::return_from_interrupt(call.cpu)
}
