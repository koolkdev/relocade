//! CPU address decoding for the PC/AT VGA adapter, independent of scanout.

use crate::Vga;
use relocade_machine::{DeviceCallbacks, DeviceConnection, IoContext, PortPolicy};

pub const OPTION_SELECT_PORT: u16 = 0x102;
/// PC/AT adapter address-decoding control (write only).
pub const ADAPTER_CONTROL_PORT: u16 = 0x46e8;

#[derive(Default)]
pub(crate) struct Setup {
    pub state: State,
}

impl Setup {
    pub fn connect(io: &mut DeviceConnection<'_>, callbacks: &DeviceCallbacks<Vga>) {
        // Setup controls remain accessible while normal adapter decoding is disabled.
        io.port(
            OPTION_SELECT_PORT,
            PortPolicy::BYTE_SEQUENCE,
            callbacks.read(|_, vga| vga.setup.state.read_option()),
            callbacks.write(write_option),
        );
        io.port(
            ADAPTER_CONTROL_PORT,
            PortPolicy::BYTE_SEQUENCE,
            |_| 0xff,
            callbacks.write(write_control),
        );
    }
}

fn write_option(context: IoContext<'_>, vga: &mut Vga, value: u8) {
    let was_enabled = vga.setup.state.enabled();
    vga.setup.state.write_option(value);
    finish_write(context, vga, was_enabled);
}

fn write_control(context: IoContext<'_>, vga: &mut Vga, value: u8) {
    let was_enabled = vga.setup.state.enabled();
    vga.setup.state.write_control(value);
    finish_write(context, vga, was_enabled);
}

fn finish_write(context: IoContext<'_>, vga: &Vga, was_enabled: bool) {
    if vga.setup.state.enabled() != was_enabled {
        vga.configure_ports(context);
        vga.programming_changed.notify(usize::MAX);
    }
}

const SETUP_MODE: u8 = 0x10;
const ADDRESSING_ENABLE: u8 = 0x08;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct State {
    control: u8,
    option: u8,
}

impl Default for State {
    fn default() -> Self {
        // Like the register banks, start in the state following firmware setup.
        Self {
            control: ADDRESSING_ENABLE,
            option: 1,
        }
    }
}

impl State {
    pub(crate) fn to_bytes(&self) -> [u8; 2] {
        [self.control, self.option]
    }

    pub(crate) fn from_bytes([control, option]: [u8; 2]) -> Option<Self> {
        (control & !(SETUP_MODE | ADDRESSING_ENABLE) == 0 && option <= 1)
            .then_some(Self { control, option })
    }

    pub(crate) fn enabled(&self) -> bool {
        self.control == ADDRESSING_ENABLE && self.option != 0
    }

    pub(crate) fn write_control(&mut self, value: u8) {
        // ROM bank selection is outside this device's memory aperture.
        self.control = value & (SETUP_MODE | ADDRESSING_ENABLE);
    }

    pub(crate) fn read_option(&self) -> u8 {
        if self.control & SETUP_MODE != 0 {
            self.option
        } else {
            0xff
        }
    }

    pub(crate) fn write_option(&mut self, value: u8) {
        // Our setup-priority convention permits IBM's adapter initialization
        // sequence: 46E8h=16h, 0102h=01h, then 46E8h=0Eh. See the README.
        if self.control & SETUP_MODE != 0 {
            self.option = value & 1;
        }
    }
}
