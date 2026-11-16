//! BIOS parallel-port configuration and INT 17h over the shared I/O registers.
//!
//! Register sequences and status polarity follow the IBM PC/AT Technical
//! Reference (March 1984), Printer BIOS, pp. 5-123–5-124. The original ROM uses
//! CPU-speed-dependent loops. Here each BDA timeout unit is one emulation-clock
//! second. Readiness comes from the connected adapter; zero checks once.

use futures_util::future::{select, Either};
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    Clock, CpuContext, IoBus, MemoryView,
};
use relocade_parallel::Port;
use std::{cell::RefCell, pin::pin, rc::Rc, time::Duration};

const BASE_ADDRESSES: u32 = 0x408;
const TIMEOUTS: u32 = 0x478;

/// INT 17h services for the adapters connected to the CPU's I/O bus.
/// Guest transfers use their registers; firmware awaits their readiness directly.
/// The live BIOS table maps logical LPT numbers to these physical adapters.
pub struct Parallel {
    clock: Clock,
    ports: Vec<Rc<RefCell<Port>>>,
}

impl Parallel {
    pub fn new(clock: Clock, ports: impl IntoIterator<Item = Rc<RefCell<Port>>>) -> Self {
        Self {
            clock,
            ports: ports.into_iter().collect(),
        }
    }

    #[interrupt(context(call), args(function = ah))]
    pub(crate) async fn dispatch(&self, call: &mut CpuContext<'_>, function: u8) {
        interrupt_dispatch!(
            call,
            match function {
                0x00 => self.send_byte,
                0x01 => self.initialize_port,
                0x02 => self.read_status,
                _ => ignore_request,
            }
        );
    }

    #[interrupt(context(memory, io), args(byte = al, index = dx), returns {
        Some(status) => { ah = status }, None => {},
    })]
    async fn send_byte(
        &self,
        memory: &MemoryView<'_>,
        io: &IoBus,
        byte: u8,
        index: u16,
    ) -> Option<u8> {
        let port = self.port(memory, index)?;
        let base = port.borrow().base();
        let control_port = base.wrapping_add(2);
        let timeout = memory
            .read_u8(TIMEOUTS + u32::from(index))
            .expect("BIOS parallel-port timeout");
        io.write_port(base, 1, u32::from(byte));
        let ready = port.borrow().wait_ready();
        let timeout = self.clock.sleep(Duration::from_secs(u64::from(timeout)));
        if let Either::Right(_) = select(pin!(ready), timeout).await {
            return Some(read_bios_status(io, base) | 1);
        }
        // The peripheral owns the handshake. BIOS toggles STROBE and leaves
        // IRQ enable and auto-feed clear, without modeling pulse widths.
        io.write_port(control_port, 1, 0x0d);
        io.write_port(control_port, 1, 0x0c);
        Some(read_bios_status(io, base))
    }

    #[interrupt(context(memory, io), args(index = dx), returns {
        Some(status) => { ah = status }, None => {},
    })]
    fn initialize_port(&self, memory: &MemoryView<'_>, io: &IoBus, index: u16) -> Option<u8> {
        let port = self.port(memory, index)?;
        let base = port.borrow().base();
        let control_port = base.wrapping_add(2);
        io.write_port(control_port, 1, 0x08);
        io.write_port(control_port, 1, 0x0c);
        Some(read_bios_status(io, base))
    }

    #[interrupt(context(memory, io), args(index = dx), returns {
        Some(status) => { ah = status }, None => {},
    })]
    fn read_status(&self, memory: &MemoryView<'_>, io: &IoBus, index: u16) -> Option<u8> {
        let port = self.port(memory, index)?;
        let base = port.borrow().base();
        Some(read_bios_status(io, base))
    }

    fn port(&self, memory: &MemoryView<'_>, index: u16) -> Option<&RefCell<Port>> {
        if index >= 3 {
            return None;
        }
        let base = memory
            .read_u16(BASE_ADDRESSES + 2 * u32::from(index))
            .expect("BIOS parallel-port base");
        if base == 0 {
            return None;
        }
        self.ports
            .iter()
            .find(|port| port.borrow().base() == base)
            .map(Rc::as_ref)
    }
}

pub(crate) fn initialize(memory: &mut MemoryView<'_>, io: &IoBus, ports: [u16; 3]) {
    for (index, base) in ports.into_iter().enumerate() {
        memory
            .write_u16(BASE_ADDRESSES + 2 * index as u32, base)
            .expect("BIOS parallel-port base");
        memory
            .write_u8(TIMEOUTS + index as u32, 20)
            .expect("BIOS parallel-port timeout");
        if base != 0 {
            // Firmware leaves INIT high, SELECTIN low and STROBE inactive.
            io.write_port(base.wrapping_add(2), 1, 0x0c);
        }
    }
}

#[interrupt]
fn ignore_request() {}

fn read_bios_status(io: &IoBus, base: u16) -> u8 {
    let status_port = base.wrapping_add(1);
    let raw_status = io.read_port(status_port, 1) as u8;
    // BIOS reports positive error and acknowledge indications; the hardware
    // register exposes their active-low connector levels. Bits 2:1 stay clear.
    (raw_status & 0xf8) ^ 0x48
}
