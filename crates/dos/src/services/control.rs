//! DOS entry scope and the return convention for an awaited break callback.

use super::Services;
use crate::state::indos;
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    registers as reg, CpuContext, SavedRegisters,
};
use relocade_x86::Segment;

const CALLBACK_CLEARED_FLAGS: u32 = (1 << 10) | (1 << 9) | 1; // CLD, CLI, CLC.

/// Owns the caller state and DOS entry scope across I/O and nested interrupts.
pub(super) struct Request<'call> {
    pub(super) services: &'call Services<'call>,
    pub(super) function: u8,
    registers: SavedRegisters<9>,
    stack: SavedRegisters<2>,
    sp: u16,
    in_dos: Option<indos::Call<'call>>,
}

impl<'call> Request<'call> {
    pub(super) async fn enter(call: &mut CpuContext<'_>, services: &'call Services<'call>) -> Self {
        let cpu = call.cpu;
        let function = cpu.ah();
        let mut request = Self {
            function,
            registers: cpu.save_registers([
                reg::ax,
                reg::bx,
                reg::cx,
                reg::dx,
                reg::si,
                reg::di,
                reg::bp,
                reg::segment(Segment::Ds),
                reg::segment(Segment::Es),
            ]),
            stack: cpu.save_registers([reg::sp, reg::segment(Segment::Ss)]),
            sp: cpu.sp(),
            // AH=33h neither enters DOS nor performs the BREAK ON precheck.
            in_dos: (function != 0x33).then(|| services.dos.indos().enter()),
            services,
        };
        if function != 0x33 && function > 0x0c && services.dos.check_break.get() {
            while super::io::check_console_break(services).await {
                request.handle_break(call).await;
            }
        }
        request
    }

    /// Returns only when the operation may restart. Process termination abandons
    /// this await, so a terminated read cannot publish an ordinary result.
    pub(super) async fn handle_break(&mut self, call: &mut CpuContext<'_>) {
        let output = self.services.stdout();
        for &byte in b"^C\r\n" {
            let _ = output.write_cooked(byte).await;
        }
        self.in_dos.take();
        self.registers.restore();
        self.stack.restore();
        call.cpu
            .set_eflags(call.cpu.eflags() & !CALLBACK_CLEARED_FLAGS);
        call.call_interrupt(crate::interrupts::CONTROL_BREAK).await;

        // IRET restores SP and ignores carry. RETF leaves the saved FLAGS word
        // on the stack and uses live carry to request termination.
        let terminate = call.cpu.sp() != self.sp && call.cpu.carry();
        self.stack.restore();
        if terminate {
            self.services.exit(&mut call.memory, Ok(0));
            std::future::pending::<()>().await;
        }
        self.in_dos = Some(self.services.dos.indos().enter());
        call.cpu.set_interrupts_enabled(true);
    }
}

/// AH=33h bypasses InDOS and ordinary break checking, including when nested.
#[interrupt(context(call), args(function = al, state = dl))]
pub(super) async fn break_setting(
    call: &mut relocade_machine::CpuContext<'_>,
    services: &Services<'_>,
    function: u8,
    state: u8,
) {
    interrupt_dispatch!(
        call,
        match function {
            0 => get_break(services),
            1 => set_break(services, state),
            2 => exchange_break(services, state),
            _ => invalid_break_function,
        }
    );
}

#[interrupt(returns { enabled => { dl = u8::from(enabled) } })]
fn get_break(services: &Services<'_>) -> bool {
    services.dos.check_break.get()
}

#[interrupt]
fn set_break(services: &Services<'_>, state: u8) {
    services.dos.check_break.set(state & 1 != 0);
}

#[interrupt(returns { previous => { dl = u8::from(previous) } })]
fn exchange_break(services: &Services<'_>, state: u8) -> bool {
    services.dos.check_break.replace(state & 1 != 0)
}

#[interrupt(returns { () => { al = 0xffu8 } })]
fn invalid_break_function() {}
