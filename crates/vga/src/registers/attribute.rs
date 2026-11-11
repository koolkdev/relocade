//! Attribute Controller register access, IBM VGA Function pages 2-89–2-96.

use super::text_mode;
use crate::{programming::display_write, Vga};
use relocade_machine::{BytePort, DeviceCallbacks, IoContext, PortWrite};

pub const ADDRESS_PORT: u16 = 0x3c0;
pub const DATA_READ_PORT: u16 = 0x3c1;

#[derive(Default)]
pub(crate) struct Attribute {
    pub state: State,
    ports: Option<Ports>,
}

struct Ports {
    address: BytePort,
    data: BytePort,
    packed_write: PortWrite,
}

impl Attribute {
    pub fn connect(&mut self, address: BytePort, data: BytePort, callbacks: &DeviceCallbacks<Vga>) {
        // Address writes also control display enable, before the data phase.
        let address = address.define(
            callbacks.read(|_, vga| vga.registers.attribute.state.address()),
            callbacks.write(display_write(write_address_or_data)),
        );
        let data = data.define(
            callbacks.read(|_, vga| vga.registers.attribute.state.read()),
            |_, _| {},
        );
        let packed_write = address.write_handler(callbacks.write(write_address_or_data));
        self.ports = Some(Ports {
            address,
            data,
            packed_write,
        });
    }

    pub fn configure(&self, mut context: IoContext<'_>, packed: bool) {
        let Some(ports) = &self.ports else { return };
        let write = if packed {
            &ports.packed_write
        } else {
            &ports.address.write
        };
        ports
            .address
            .configure(context.reborrow(), &ports.address.read, write);
        ports
            .data
            .configure(context, &ports.data.read, &ports.data.write);
    }
}

fn write_address_or_data(_: IoContext<'_>, vga: &mut Vga, value: u8) {
    vga.registers.attribute.state.write(value);
}

pub const MODE_CONTROL: u8 = 0x10;
pub const OVERSCAN_COLOR: u8 = 0x11;
pub const COLOR_PLANE_ENABLE: u8 = 0x12;
pub const HORIZONTAL_PANNING: u8 = 0x13;
pub const COLOR_SELECT: u8 = 0x14;
pub const PALETTE_ADDRESS_SOURCE: u8 = 0x20;

/// Attribute Controller values, raw address byte and CPU access phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    address: u8,
    data_next: bool,
    pub values: [u8; 0x15],
}

impl Default for State {
    fn default() -> Self {
        let mut values = [0; 0x15];
        values[..16].copy_from_slice(&text_mode::ATTRIBUTE_PALETTE);
        for &(index, value) in text_mode::ATTRIBUTE_REGISTERS {
            values[usize::from(index)] = value;
        }
        Self {
            address: PALETTE_ADDRESS_SOURCE,
            data_next: false,
            values,
        }
    }
}

impl State {
    pub fn address(&self) -> u8 {
        self.address
    }

    pub(crate) fn data_next(&self) -> bool {
        self.data_next
    }

    fn register(&self, index: u8) -> u8 {
        self.values[usize::from(index)]
    }

    pub(crate) fn read(&self) -> u8 {
        let index = self.address & 0x1f;
        if index <= COLOR_SELECT {
            self.register(index)
        } else {
            0xff
        }
    }

    /// An active Input Status 1 read explicitly selects the address phase.
    pub(crate) fn reset_address(&mut self) {
        self.data_next = false;
    }

    pub(crate) fn write(&mut self, value: u8) {
        // Every write advances the phase, including ignored register writes.
        let data_phase = self.data_next;
        self.data_next = !data_phase;
        if !data_phase {
            self.address = value & 0x3f;
            return;
        }
        let index = self.address & 0x1f;
        let mask = match index {
            // IBM Type 1 disables palette access while display addressing is active.
            0..=0x0f if self.address & PALETTE_ADDRESS_SOURCE == 0 => 0x3f,
            MODE_CONTROL => 0xef,
            OVERSCAN_COLOR => 0xff,
            COLOR_PLANE_ENABLE | HORIZONTAL_PANNING | COLOR_SELECT => 0x0f,
            _ => return,
        };
        self.values[usize::from(index)] = value & mask;
    }
}
