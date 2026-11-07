//! Fixed PC wiring and cascaded acknowledgement; each chip owns its registers.

use crate::{
    controller::{Controller, IRQ_COUNT},
    CASCADE_IRQ, COMMAND_PORT, MASK_PORT, SLAVE_COMMAND_PORT, SLAVE_MASK_PORT,
};
use std::task::Waker;

pub(super) struct Pair {
    pub master: Controller,
    slave: Controller,
    pub waker: Option<Waker>,
}

impl Pair {
    pub fn new(master_vector_base: u8, slave_vector_base: u8) -> Self {
        Self {
            master: Controller::new(master_vector_base, 1 << CASCADE_IRQ),
            slave: Controller::new(slave_vector_base, CASCADE_IRQ),
            waker: None,
        }
    }

    pub fn chip(&mut self, irq: u8) -> &mut Controller {
        if irq < IRQ_COUNT {
            &mut self.master
        } else {
            &mut self.slave
        }
    }

    pub fn refresh_cascade(&mut self) {
        self.master
            .set_level(CASCADE_IRQ, self.slave.next_irq().is_some());
    }

    pub fn acknowledge(&mut self) -> Option<u8> {
        let irq = self.master.acknowledge()?;
        if self.master.single || self.master.cascade & (1 << irq) == 0 {
            return Some(self.master.vector_base | irq);
        }
        if self.slave.single || self.slave.cascade & 7 != irq {
            // No slave drives the vector bus for this programmed cascade ID.
            return Some(0xff);
        }
        // INTA lowers the slave INT output, even when another request becomes
        // eligible immediately (for example with automatic EOI).
        self.master.set_level(CASCADE_IRQ, false);
        let slave_irq = self.slave.acknowledge().unwrap_or(7);
        // A withdrawn slave request produces IRQ15 without setting slave ISR7.
        Some(self.slave.vector_base | slave_irq)
    }

    pub fn read_port(&mut self, port: u16) -> u32 {
        match port {
            COMMAND_PORT => self.master.read_command().into(),
            MASK_PORT => self.master.mask.into(),
            SLAVE_COMMAND_PORT => {
                // Polling acknowledges just the addressed chip. Software must
                // poll both chips to service a cascaded request this way.
                if self.slave.poll_pending {
                    self.master.set_level(CASCADE_IRQ, false);
                }
                self.slave.read_command().into()
            }
            SLAVE_MASK_PORT => self.slave.mask.into(),
            _ => u32::MAX,
        }
    }

    pub fn write_port(&mut self, port: u16, value: u8) {
        match port {
            COMMAND_PORT => self.master.command(value),
            MASK_PORT => self.master.data(value),
            SLAVE_COMMAND_PORT => self.slave.command(value),
            SLAVE_MASK_PORT => self.slave.data(value),
            _ => {}
        }
    }
}
