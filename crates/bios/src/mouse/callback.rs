//! IBM's packet callback convention over the shared guest-call mechanism.

use relocade_machine::{registers as reg, CpuContext, FarPointer};
use relocade_x86::Segment;

pub(super) async fn invoke(call: &mut CpuContext<'_>, target: FarPointer, packet: [u8; 3]) {
    let [status, x, y] = packet;
    call.cpu
        .with_saved_registers(
            [
                reg::eax,
                reg::ebx,
                reg::ecx,
                reg::edx,
                reg::esi,
                reg::edi,
                reg::ebp,
                reg::segment(Segment::Ds),
                reg::segment(Segment::Es),
                reg::segment(Segment::Fs),
                reg::segment(Segment::Gs),
            ],
            async {
                call.call_far(target, &[u16::from(status), u16::from(x), u16::from(y), 0])
                    .await;
            },
        )
        .await;
}
