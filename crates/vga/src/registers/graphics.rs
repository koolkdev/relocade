//! Graphics Controller registers, IBM VGA Function pages 2-78–2-88.

use super::text_mode;
use crate::{
    programming::{RegisterWrites, WriteEffect},
    Vga,
};
use relocade_machine::{DeviceCallbacks, IndexedPorts, IoContext};

pub const INDEX_PORT: u16 = 0x3ce;
pub const DATA_PORT: u16 = 0x3cf;

#[derive(Default)]
pub(crate) struct Graphics {
    pub state: State,
    ports: Option<Ports>,
}

struct Ports {
    pair: IndexedPorts<u8>,
    writes: RegisterWrites,
}

impl Graphics {
    pub fn connect(&mut self, ports: IndexedPorts<u8>, callbacks: &DeviceCallbacks<Vga>) {
        let mut pair = ports.index(
            callbacks.read(|_, vga| vga.registers.graphics.state.index()),
            callbacks.write(select_index),
        );
        pair.data.read = pair
            .data
            .read_handler(callbacks.read(|_, vga| vga.registers.graphics.state.read()));
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
        if self.state.index <= BIT_MASK {
            WriteEffect::Display
        } else {
            WriteEffect::RegistersOnly
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
    if vga.registers.graphics.state.select(value) {
        let packed = vga.dispi.state.display_layout().is_some();
        vga.registers.graphics.configure_write(context, packed);
    }
}

fn write_data(_: IoContext<'_>, vga: &mut Vga, value: u8) {
    vga.registers.graphics.state.write(value);
}

pub const SET_RESET: u8 = 0x00;
pub const ENABLE_SET_RESET: u8 = 0x01;
pub const COLOR_COMPARE: u8 = 0x02;
pub const DATA_ROTATE: u8 = 0x03;
pub const READ_MAP_SELECT: u8 = 0x04;
pub const MODE: u8 = 0x05;
pub const MISCELLANEOUS: u8 = 0x06;
pub const COLOR_DONT_CARE: u8 = 0x07;
pub const BIT_MASK: u8 = 0x08;

/// Graphics Controller values and the current CPU register index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    index: u8,
    pub values: [u8; 9],
}

impl Default for State {
    fn default() -> Self {
        let mut values = [0; 9];
        for &(index, value) in text_mode::GRAPHICS_REGISTERS {
            values[usize::from(index)] = value;
        }
        Self { index: 0, values }
    }
}

impl State {
    pub fn index(&self) -> u8 {
        self.index
    }

    pub(crate) fn select(&mut self, index: u8) -> bool {
        let index = index & 0x0f;
        let changed = self.index != index;
        self.index = index;
        changed
    }

    fn register(&self, index: u8) -> u8 {
        self.values[usize::from(index)]
    }

    pub(crate) fn read(&self) -> u8 {
        if self.index <= BIT_MASK {
            self.register(self.index)
        } else {
            0xff
        }
    }

    pub(crate) fn write(&mut self, value: u8) {
        let mask = match self.index {
            SET_RESET | ENABLE_SET_RESET | COLOR_COMPARE | MISCELLANEOUS | COLOR_DONT_CARE => 0x0f,
            DATA_ROTATE => 0x1f,
            READ_MAP_SELECT => 0x03,
            MODE => 0x7b,
            BIT_MASK => 0xff,
            _ => return,
        };
        self.values[usize::from(self.index)] = value & mask;
    }

    pub(crate) fn memory_offset(&self, address: u32) -> Option<u32> {
        let (start, size) = match (self.register(MISCELLANEOUS) >> 2) & 3 {
            0 => (0xa0000, 0x20000),
            1 => (0xa0000, 0x10000),
            2 => (0xb0000, 0x08000),
            _ => (0xb8000, 0x08000),
        };
        address.checked_sub(start).filter(|offset| *offset < size)
    }

    pub(crate) fn odd_even_reads(&self) -> bool {
        self.register(MODE) & 0x10 != 0
    }

    pub(crate) fn chain_odd_even(&self) -> bool {
        self.register(MISCELLANEOUS) & 2 != 0
    }

    pub(crate) fn read_plane(&self) -> usize {
        usize::from(self.register(READ_MAP_SELECT))
    }

    pub(crate) fn compare_reads(&self) -> bool {
        self.register(MODE) & 8 != 0
    }

    pub(crate) fn compare(&self, latches: [u8; 4]) -> u8 {
        let mut matches = 0xff;
        for (plane, latch) in latches.into_iter().enumerate() {
            if self.register(COLOR_DONT_CARE) & (1 << plane) != 0 {
                let color = if self.register(COLOR_COMPARE) & (1 << plane) != 0 {
                    0xff
                } else {
                    0
                };
                matches &= !(latch ^ color);
            }
        }
        matches
    }

    /// The CPU read latches supply both logical-operation input and masked bits.
    /// A write never reloads them from the destination address.
    pub(crate) fn write_data(&self, value: u8, latches: [u8; 4]) -> [u8; 4] {
        let mode = self.register(MODE) & 3;
        if mode == 1 {
            return latches;
        }
        let rotated = value.rotate_right(u32::from(self.register(DATA_ROTATE) & 7));
        let mask = self.register(BIT_MASK) & if mode == 3 { rotated } else { 0xff };
        std::array::from_fn(|plane| {
            let set_reset = if self.register(SET_RESET) & (1 << plane) != 0 {
                0xff
            } else {
                0
            };
            let source = match mode {
                0 if self.register(ENABLE_SET_RESET) & (1 << plane) != 0 => set_reset,
                0 => rotated,
                2 => {
                    if value & (1 << plane) != 0 {
                        0xff
                    } else {
                        0
                    }
                }
                _ => set_reset,
            };
            let latch = latches[plane];
            let combined = match (self.register(DATA_ROTATE) >> 3) & 3 {
                0 => source,
                1 => source & latch,
                2 => source | latch,
                _ => source ^ latch,
            };
            (combined & mask) | (latch & !mask)
        })
    }
}
