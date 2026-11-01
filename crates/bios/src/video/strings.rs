//! INT 10h string formats and cursor-update policy.
use super::unsupported;
use crate::screen::Output;

pub(super) fn interrupt(call: &mut crate::BiosContext<'_>) {
    let format = call.cpu.al();
    if format > 3 {
        return unsupported(call);
    }
    let count = call.cpu.cx();
    if count == 0 {
        return;
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
}
