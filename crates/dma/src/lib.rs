//! Primary 8237A DMA registers and the PC/AT's 8-bit channel page registers.
#![forbid(unsafe_code)]

use relocade_machine::{Device, DeviceCallbacks, DeviceConnection, PortRead, PortWrite, Runner};
use std::{cell::RefCell, rc::Rc};

pub const ADDRESS_PORTS: [u16; 4] = [0x00, 0x02, 0x04, 0x06];
pub const COUNT_PORTS: [u16; 4] = [0x01, 0x03, 0x05, 0x07];
pub const PAGE_PORTS: [u16; 4] = [0x87, 0x83, 0x81, 0x82];
pub const STATUS_PORT: u16 = 0x08;
pub const COMMAND_PORT: u16 = 0x08;
pub const REQUEST_PORT: u16 = 0x09;
pub const SINGLE_MASK_PORT: u16 = 0x0a;
pub const MODE_PORT: u16 = 0x0b;
pub const CLEAR_FLIP_FLOP_PORT: u16 = 0x0c;
pub const TEMPORARY_PORT: u16 = 0x0d;
pub const MASTER_CLEAR_PORT: u16 = 0x0d;
pub const CLEAR_MASK_PORT: u16 = 0x0e;
pub const MASK_PORT: u16 = 0x0f;

#[derive(Clone, Copy, Default)]
struct Register {
    base: u16,
    current: u16,
}

impl Register {
    fn write(&mut self, shift: u32, value: u8) {
        for word in [&mut self.base, &mut self.current] {
            *word = (*word & !(0xff << shift)) | (u16::from(value) << shift);
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Channel {
    address: Register,
    count: Register,
    page: u8,
    mode: u8,
}

/// Execution-owned primary DMA controller. Channels start masked.
/// The external page registers retain the upper eight address bits.
pub struct Dma {
    channels: [Channel; 4],
    command: u8,
    mask: u8,
    software_requests: u8,
    terminal_count: u8,
    high_byte: bool,
}

impl Default for Dma {
    fn default() -> Self {
        Self {
            channels: [Channel::default(); 4],
            command: 0,
            mask: 0x0f,
            software_requests: 0,
            terminal_count: 0,
            high_byte: false,
        }
    }
}

impl Dma {
    pub async fn initialize(runner: &Runner) -> Rc<RefCell<Self>> {
        let dma = Rc::new(RefCell::new(Self::default()));
        runner
            .connect(dma.clone(), |io| Self::configure_io(io, &dma))
            .await;
        dma
    }

    pub fn configure_io(io: &mut DeviceConnection<'_>, dma: &Rc<RefCell<Self>>) {
        let callbacks = DeviceCallbacks::new(dma);
        let unreadable = PortRead::byte(|_| 0xff);
        for channel in 0..4 {
            for (register, port) in [
                (channel * 2, ADDRESS_PORTS[channel]),
                (channel * 2 + 1, COUNT_PORTS[channel]),
            ] {
                io.bind(
                    port,
                    &PortRead::byte(callbacks.read(move |_, dma| dma.read_word(register))),
                    &PortWrite::byte(
                        callbacks.write(move |_, dma, value| dma.write_word(register, value)),
                    ),
                );
            }
            io.bind(
                PAGE_PORTS[channel],
                &PortRead::byte(callbacks.read(move |_, dma| dma.channels[channel].page)),
                &PortWrite::byte(
                    callbacks.write(move |_, dma, value| dma.channels[channel].page = value),
                ),
            );
        }
        io.bind(
            STATUS_PORT,
            &PortRead::byte(callbacks.read(|_, dma| dma.read_status())),
            &PortWrite::byte(callbacks.write(|_, dma, value| dma.command = value)),
        );
        io.bind(
            REQUEST_PORT,
            &unreadable,
            &PortWrite::byte(callbacks.write(|_, dma, value| {
                set_bit(&mut dma.software_requests, value & 3, value & 4 != 0);
            })),
        );
        io.bind(
            SINGLE_MASK_PORT,
            &unreadable,
            &PortWrite::byte(callbacks.write(|_, dma, value| {
                set_bit(&mut dma.mask, value & 3, value & 4 != 0);
            })),
        );
        io.bind(
            MODE_PORT,
            &unreadable,
            &PortWrite::byte(callbacks.write(|_, dma, value| {
                dma.channels[usize::from(value & 3)].mode = value & 0xfc;
            })),
        );
        io.bind(
            CLEAR_FLIP_FLOP_PORT,
            &unreadable,
            &PortWrite::byte(callbacks.write(|_, dma, _| dma.high_byte = false)),
        );
        io.bind(
            MASTER_CLEAR_PORT,
            &PortRead::byte(|_| 0), // No memory-to-memory transfers use the temporary register.
            &PortWrite::byte(callbacks.write(|_, dma, _| dma.master_clear())),
        );
        io.bind(
            CLEAR_MASK_PORT,
            &unreadable,
            &PortWrite::byte(callbacks.write(|_, dma, _| dma.mask = 0)),
        );
        io.bind(
            MASK_PORT,
            &unreadable,
            &PortWrite::byte(callbacks.write(|_, dma, value| dma.mask = value & 0x0f)),
        );
    }

    fn word(&mut self, register: usize) -> &mut Register {
        let channel = &mut self.channels[register / 2];
        if register & 1 == 0 {
            &mut channel.address
        } else {
            &mut channel.count
        }
    }

    fn byte_shift(&mut self) -> u32 {
        let shift = u32::from(self.high_byte) * 8;
        self.high_byte = !self.high_byte;
        shift
    }

    fn read_word(&mut self, register: usize) -> u8 {
        let shift = self.byte_shift();
        (self.word(register).current >> shift) as u8
    }

    fn write_word(&mut self, register: usize, value: u8) {
        let shift = self.byte_shift();
        self.word(register).write(shift, value);
    }

    fn read_status(&mut self) -> u8 {
        let status = (self.software_requests << 4) | self.terminal_count;
        self.terminal_count = 0;
        status
    }

    fn master_clear(&mut self) {
        self.command = 0;
        self.mask = 0x0f;
        self.software_requests = 0;
        self.terminal_count = 0;
        self.high_byte = false;
    }
}

fn set_bit(bits: &mut u8, channel: u8, set: bool) {
    let bit = 1 << channel;
    if set {
        *bits |= bit;
    } else {
        *bits &= !bit;
    }
}

impl Device for Dma {}

#[cfg(test)]
mod tests;
