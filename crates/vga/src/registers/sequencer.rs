//! Sequencer registers, IBM VGA Function pages 2-47–2-50.

use super::text_mode;
use crate::{
    programming::{RegisterWrites, WriteEffect},
    Vga,
};
use relocade_machine::{DeviceCallbacks, IndexedPorts, IoContext};

pub const INDEX_PORT: u16 = 0x3c4;
pub const DATA_PORT: u16 = 0x3c5;

#[derive(Default)]
pub(crate) struct Sequencer {
    pub state: State,
    ports: Option<Ports>,
}

struct Ports {
    pair: IndexedPorts<u8>,
    writes: RegisterWrites,
}

impl Sequencer {
    pub fn connect(&mut self, ports: IndexedPorts<u8>, callbacks: &DeviceCallbacks<Vga>) {
        let mut pair = ports.index(
            callbacks.read(|_, vga| vga.registers.sequencer.state.index()),
            callbacks.write(select_index),
        );
        pair.data.read = pair
            .data
            .read_handler(callbacks.read(|_, vga| vga.registers.sequencer.state.read()));
        let writes = RegisterWrites::new(&pair.data, callbacks, write_data);
        self.ports = Some(Ports { pair, writes });
    }

    pub fn configure(&self, context: IoContext<'_>, packed: bool) {
        let Some(ports) = &self.ports else { return };
        ports.pair.configure(
            context,
            &ports.pair.data.read,
            ports.writes.get(self.write_effect(packed)),
        );
    }

    fn write_effect(&self, packed: bool) -> WriteEffect {
        if packed {
            return WriteEffect::RegistersOnly;
        }
        match self.state.index {
            RESET | CLOCKING_MODE => WriteEffect::Timing,
            MAP_MASK..=MEMORY_MODE => WriteEffect::Display,
            _ => WriteEffect::RegistersOnly,
        }
    }

    fn configure_write(&self, context: IoContext<'_>, packed: bool) {
        let Some(ports) = &self.ports else { return };
        ports
            .pair
            .data
            .set_write(context, ports.writes.get(self.write_effect(packed)));
    }
}

fn select_index(context: IoContext<'_>, vga: &mut Vga, value: u8) {
    if vga.registers.sequencer.state.select(value) {
        let packed = vga.dispi.state.display_layout().is_some();
        vga.registers.sequencer.configure_write(context, packed);
    }
}

fn write_data(_: IoContext<'_>, vga: &mut Vga, value: u8) {
    vga.registers.sequencer.state.write(value);
}

pub const RESET: u8 = 0x00;
pub const CLOCKING_MODE: u8 = 0x01;
pub const MAP_MASK: u8 = 0x02;
pub const CHARACTER_MAP_SELECT: u8 = 0x03;
pub const MEMORY_MODE: u8 = 0x04;

/// Clocking Mode bit 5 blanks output while leaving raster timing running.
pub const SCREEN_OFF: u8 = 0x20;

/// Reset register value: hold synchronous reset, release asynchronous reset.
pub const SYNCHRONOUS_RESET: u8 = 0x01;
/// Reset register value: release both active-low reset inputs.
pub const RESET_RELEASED: u8 = 0x03;

/// Sequencer values and the current CPU register index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    index: u8,
    pub values: [u8; 5],
}

impl Default for State {
    fn default() -> Self {
        let mut values = [0; 5];
        for &(index, value) in text_mode::SEQUENCER_REGISTERS {
            values[usize::from(index)] = value;
        }
        Self {
            index: RESET,
            values,
        }
    }
}

impl State {
    pub fn index(&self) -> u8 {
        self.index
    }

    pub(crate) fn select(&mut self, index: u8) -> bool {
        let index = index & 7;
        let changed = self.index != index;
        self.index = index;
        changed
    }

    pub(crate) fn read(&self) -> u8 {
        self.values
            .get(usize::from(self.index))
            .copied()
            .unwrap_or(0xff)
    }

    fn register(&self, index: u8) -> u8 {
        self.values[usize::from(index)]
    }

    pub(crate) fn write(&mut self, value: u8) {
        let mask = match self.index {
            RESET => RESET_RELEASED,
            CLOCKING_MODE => 0x3d,
            CHARACTER_MAP_SELECT => 0x3f,
            MAP_MASK => 0x0f,
            MEMORY_MODE => 0x0e,
            _ => return,
        };
        self.values[usize::from(self.index)] = value & mask;
    }

    pub(crate) fn chain_four(&self) -> bool {
        self.register(MEMORY_MODE) & 8 != 0
    }

    pub(crate) fn odd_even_writes(&self) -> bool {
        self.register(MEMORY_MODE) & 4 == 0
    }

    pub(crate) fn extended_memory(&self) -> bool {
        self.register(MEMORY_MODE) & 2 != 0
    }

    pub(crate) fn write_planes(&self, offset: u32) -> u8 {
        let addressed = if self.chain_four() {
            1 << (offset & 3)
        } else if self.odd_even_writes() {
            0x05 << (offset & 1)
        } else {
            0x0f
        };
        self.register(MAP_MASK) & addressed
    }

    pub(crate) fn reset_active(&self) -> bool {
        self.register(RESET) != RESET_RELEASED
    }

    pub(crate) fn dots_per_character(&self) -> u64 {
        // SR01 bit 0 selects eight dots when set, nine when clear.
        if self.register(CLOCKING_MODE) & 1 == 0 {
            9
        } else {
            8
        }
    }

    pub(crate) fn dot_clock_divisor(&self) -> u64 {
        // SR01 bit 3 divides the oscillator by two before generating dot clocks.
        if self.register(CLOCKING_MODE) & 8 == 0 {
            1
        } else {
            2
        }
    }
}
