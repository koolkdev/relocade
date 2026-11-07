//! IRQ1 scan consumption and the BIOS Ctrl-Break callback convention.

use super::{KeyStroke, Keyboard, Scan};
use relocade_machine::{registers as reg, CpuContext};
use relocade_x86::{Segment, SegmentRegister};

const DIRECTION_FLAG: u32 = 1 << 10;

pub(crate) async fn interrupt(call: &mut CpuContext<'_>, keyboard: &Keyboard) {
    let Some(byte) = relocade_at_keyboard::read_scan_code(call.io) else {
        return;
    };
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
