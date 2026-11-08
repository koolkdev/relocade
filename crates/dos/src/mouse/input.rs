//! IBM's incoming packet ABI and Microsoft's outgoing register callback ABI.

use super::{Mouse, PACKET_ENTRY};
use relocade_machine::{registers as reg, CpuContext, FarPointer, Instant};
use relocade_x86::{Segment, SegmentRegister};

const BIOS_MOUSE_INITIALIZE: u16 = 0xc205;
const BIOS_MOUSE_SET_CALLBACK: u16 = 0xc207;
const BIOS_MOUSE_SET_ENABLED: u16 = 0xc200;
const X_NEGATIVE: u8 = 1 << 4;
const Y_NEGATIVE: u8 = 1 << 5;

impl Mouse {
    pub(super) async fn initialize(&self, call: &mut CpuContext<'_>) -> bool {
        self.end_process();
        let coordinates = self.coordinates();
        *self.state.borrow_mut() = super::state::State::new(coordinates);
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
                    for (function, parameter) in [
                        (BIOS_MOUSE_INITIALIZE, 3 << 8),
                        (BIOS_MOUSE_SET_CALLBACK, PACKET_ENTRY.offset),
                        (BIOS_MOUSE_SET_ENABLED, 1 << 8),
                    ] {
                        call.cpu.set_ax(function);
                        call.cpu.set_bx(parameter);
                        call.cpu.set_segment(
                            Segment::Es,
                            SegmentRegister::real_mode(Segment::Es, PACKET_ENTRY.segment),
                        );
                        call.call_interrupt(relocade_bios::interrupts::SYSTEM).await;
                        if call.cpu.eflags() & 1 != 0 {
                            return false;
                        }
                    }
                    true
                },
            )
            .await
    }

    pub(super) async fn receive(&self, call: &mut CpuContext<'_>, now: Instant) {
        // RETF's IP:CS precede IBM's Z, Y, X and status words. These are the
        // actual BIOS arguments, not a second buffer or a private guest protocol.
        let stack = call.cpu.segment(Segment::Ss);
        let mut arguments = [0; 8];
        call.memory
            .read_segment(
                &stack,
                u32::from(call.cpu.sp()) + FarPointer::BYTE_LEN as u32,
                &mut arguments,
            )
            .expect("BIOS packet arguments");
        let [_, _, y, _, x, _, status, _] = arguments;
        let x = signed_count(x, status & X_NEGATIVE != 0);
        let y = -signed_count(y, status & Y_NEGATIVE != 0);
        let coordinates = self.coordinates();
        let (event, callback) = {
            let mut state = self.state.borrow_mut();
            (
                state.packet(coordinates, now, u16::from(status & 7), x, y),
                state.callback,
            )
        };
        self.update_cursor();
        if event.mask & callback.mask != 0 && callback.target.packed() != 0 {
            // BIOS already owns preservation of the interrupted registers. Its
            // scope also covers this user's callback, including nested services.
            call.cpu.set_ax(event.mask);
            call.cpu.set_bx(event.buttons);
            call.cpu.set_cx(event.position.x);
            call.cpu.set_dx(event.position.y);
            call.cpu.set_si(event.motion[0] as u16);
            call.cpu.set_di(event.motion[1] as u16);
            call.call_far(callback.target, &[]).await;
        }
    }
}

fn signed_count(byte: u8, negative: bool) -> i16 {
    i16::from(byte) - if negative { 256 } else { 0 }
}
