//! The real-mode register ABI exposed to a Rust DOS process.

use relocade_machine::CpuStateAccess;
use relocade_x86::{FlagsState, Segment, SegmentRegister};

/// Arguments and results of a 16-bit software interrupt. The process owns CS:IP
/// and SS:SP. Default flags enable hardware interrupts and clear arithmetic flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptRegisters {
    pub ax: u16,
    pub bx: u16,
    pub cx: u16,
    pub dx: u16,
    pub si: u16,
    pub di: u16,
    pub bp: u16,
    pub ds: u16,
    pub es: u16,
    pub flags: u16,
}

impl Default for InterruptRegisters {
    fn default() -> Self {
        let mut flags = FlagsState::default();
        flags.bytes.if_ = 1;
        Self {
            ax: 0,
            bx: 0,
            cx: 0,
            dx: 0,
            si: 0,
            di: 0,
            bp: 0,
            ds: 0,
            es: 0,
            flags: flags.eflags() as u16,
        }
    }
}

impl InterruptRegisters {
    /// DOS reports most service errors with CF set and the error code in AX.
    pub fn carry(&self) -> bool {
        self.flags & 1 != 0
    }

    pub(super) fn read(cpu: &CpuStateAccess) -> Self {
        Self {
            ax: cpu.ax(),
            bx: cpu.bx(),
            cx: cpu.cx(),
            dx: cpu.dx(),
            si: cpu.si(),
            di: cpu.di(),
            bp: cpu.bp(),
            ds: cpu.segment(Segment::Ds).selector,
            es: cpu.segment(Segment::Es).selector,
            flags: cpu.eflags() as u16,
        }
    }

    pub(super) fn write(self, cpu: &CpuStateAccess) {
        cpu.set_ax(self.ax);
        cpu.set_bx(self.bx);
        cpu.set_cx(self.cx);
        cpu.set_dx(self.dx);
        cpu.set_si(self.si);
        cpu.set_di(self.di);
        cpu.set_bp(self.bp);
        cpu.set_segment(
            Segment::Ds,
            SegmentRegister::real_mode(Segment::Ds, self.ds),
        );
        cpu.set_segment(
            Segment::Es,
            SegmentRegister::real_mode(Segment::Es, self.es),
        );
        cpu.set_eflags(u32::from(self.flags));
    }
}
