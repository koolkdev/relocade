//! INT 10h string formats and cursor-update policy.
use crate::screen::Output;

pub(super) fn interrupt(call: &mut crate::BiosContext<'_>) -> Option<()> {
    let format = call.cpu.al();
    if format > 3 {
        return None;
    }
    let count = call.cpu.cx();
    if count == 0 {
        return Some(());
    }
    let mut output = Output::new(&mut call.memory, call.io, call.cpu.bh())?;
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
    Some(())
}
