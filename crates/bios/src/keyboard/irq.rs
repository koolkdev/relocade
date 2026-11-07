//! IRQ1 scan consumption and the BIOS Ctrl-Break callback convention.

use super::{KeyStroke, Keyboard, Scan};
use futures_util::future::select;
use relocade_at_keyboard::{
    DISABLE_INTERFACE, ENABLE_INTERFACE, INPUT_BUFFER_FULL, STATUS_COMMAND_PORT,
};
use relocade_machine::{registers as reg, Clock, CpuContext, IoBus};
use relocade_x86::{Segment, SegmentRegister};
use std::{future::Future, pin::pin, time::Duration};

const DIRECTION_FLAG: u32 = 1 << 10;

// IBM's BIOS bounds controller polling with a 16-bit loop. Its elapsed time
// depends on CPU speed; this BIOS uses a nominal bound on the shared device clock.
const COMMAND_TIMEOUT: Duration = Duration::from_millis(500);

/// Inhibit keyboard transmission until the IRQ body, including EOI, completes.
/// The caller keeps IF clear around the latch read and final enable command.
/// Abandoning a guest call does not run a synthetic completion from Drop.
pub(crate) async fn with_keyboard_inhibited<F: Future>(
    io: &IoBus,
    clock: &impl Clock,
    body: F,
) -> F::Output {
    controller_command(io, clock, DISABLE_INTERFACE).await;
    let result = body.await;
    controller_command(io, clock, ENABLE_INTERFACE).await;
    result
}

async fn controller_command(io: &IoBus, clock: &impl Clock, command: u8) {
    let deadline = clock.now().saturating_add(COMMAND_TIMEOUT);
    wait_input_ready(io, clock, deadline).await;
    // Like SHIP_IT in the AT BIOS, attempt the write even if readiness timed out.
    // IRQ1 still reads the latch and completes when a controller is unresponsive.
    io.write_port(STATUS_COMMAND_PORT, 1, u32::from(command));
    wait_input_ready(io, clock, deadline).await;
}

async fn wait_input_ready(io: &IoBus, clock: &impl Clock, deadline: Duration) {
    let ready = io.wait_port(STATUS_COMMAND_PORT, 1, |status| {
        status & u32::from(INPUT_BUFFER_FULL) == 0
    });
    let _ = select(pin!(ready), pin!(clock.wait_until(deadline))).await;
}

pub(crate) async fn process_scan(call: &mut CpuContext<'_>, keyboard: &Keyboard, byte: u8) {
    if keyboard.scan_code(byte) != Scan::ControlBreak {
        return;
    }
    keyboard.0.borrow().data.control_break();

    // The AT IRQ1 handler saves these 16-bit registers around INT 1Bh.
    call.cpu
        .with_saved_registers(
            [
                reg::ax,
                reg::bx,
                reg::cx,
                reg::dx,
                reg::si,
                reg::di,
                reg::bp,
                reg::segment(Segment::Ds),
                reg::segment(Segment::Es),
            ],
            async {
                call.cpu
                    .set_segment(Segment::Ds, SegmentRegister::real_mode(Segment::Ds, 0x40));
                call.cpu.set_eflags(call.cpu.eflags() & !DIRECTION_FLAG);
                call.call_interrupt(crate::interrupts::CONTROL_BREAK).await;
                let _ = keyboard.store_key(KeyStroke {
                    character: 0,
                    scan_code: 0,
                });
            },
        )
        .await;
}

#[cfg(test)]
mod tests;
