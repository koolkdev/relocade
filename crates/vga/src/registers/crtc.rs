//! CRTC register storage and field decoding. Indices follow IBM VGA Function
//! pages 2-55–2-76. This owner stores register state, not live raster counters.

use super::text_mode;
use crate::{
    programming::{RegisterWrites, WriteEffect},
    Vga,
};
use relocade_machine::{DeviceCallbacks, IndexedPorts, IoContext};
use std::ops::Range;

pub const MONO_INDEX_PORT: u16 = 0x3b4;
pub const MONO_DATA_PORT: u16 = 0x3b5;
pub const COLOR_INDEX_PORT: u16 = 0x3d4;
pub const COLOR_DATA_PORT: u16 = 0x3d5;

#[derive(Default)]
pub(crate) struct Crtc {
    pub state: State,
    ports: Option<[Ports; 2]>,
}

struct Ports {
    pair: IndexedPorts<u8>,
    writes: RegisterWrites,
}

impl Crtc {
    pub fn connect(
        &mut self,
        mono: IndexedPorts<u8>,
        color: IndexedPorts<u8>,
        callbacks: &DeviceCallbacks<Vga>,
    ) {
        self.ports = Some([
            Ports::new(mono, callbacks, false),
            Ports::new(color, callbacks, true),
        ]);
    }

    pub fn configure_alias(&self, context: IoContext<'_>, color: bool, packed: bool) {
        let Some(aliases) = &self.ports else { return };
        let ports = &aliases[usize::from(color)];
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
            HORIZONTAL_TOTAL
            | HORIZONTAL_DISPLAY_END
            | HORIZONTAL_BLANK_END
            | VERTICAL_TOTAL
            | OVERFLOW
            | VERTICAL_RETRACE_START
            | VERTICAL_RETRACE_END
            | VERTICAL_DISPLAY_END
            | MODE_CONTROL => WriteEffect::Timing,
            HORIZONTAL_TOTAL..=LINE_COMPARE => WriteEffect::Display,
            _ => WriteEffect::RegistersOnly,
        }
    }

    fn configure_write(&self, context: IoContext<'_>, color: bool, packed: bool) {
        let Some(aliases) = &self.ports else { return };
        let ports = &aliases[usize::from(color)];
        ports
            .pair
            .data
            .set_write(context, ports.writes.get(self.write_effect(packed)));
    }
}

impl Ports {
    fn new(ports: IndexedPorts<u8>, callbacks: &DeviceCallbacks<Vga>, color: bool) -> Self {
        let mut pair = ports.index(
            callbacks.read(|_, vga| vga.registers.crtc.state.index()),
            callbacks.write(move |context, vga, value| {
                if vga.registers.crtc.state.select(value) {
                    let packed = vga.dispi.state.display_layout().is_some();
                    // Only this active alias may publish a data writer. The other
                    // alias is configured by its window when decoding changes.
                    vga.registers.crtc.configure_write(context, color, packed);
                }
            }),
        );
        pair.data.read = pair
            .data
            .read_handler(callbacks.read(|_, vga| vga.registers.crtc.state.read()));
        let writes = RegisterWrites::new(&pair.data, callbacks, write_data);
        Self { pair, writes }
    }
}

fn write_data(_: IoContext<'_>, vga: &mut Vga, value: u8) {
    vga.registers.crtc.state.write(value);
}

pub const HORIZONTAL_TOTAL: u8 = 0x00;
pub const HORIZONTAL_DISPLAY_END: u8 = 0x01;
pub const HORIZONTAL_BLANK_START: u8 = 0x02;
pub const HORIZONTAL_BLANK_END: u8 = 0x03;
pub const HORIZONTAL_RETRACE_START: u8 = 0x04;
pub const HORIZONTAL_RETRACE_END: u8 = 0x05;
pub const VERTICAL_TOTAL: u8 = 0x06;
pub const OVERFLOW: u8 = 0x07;
pub const PRESET_ROW_SCAN: u8 = 0x08;
pub const MAXIMUM_SCAN_LINE: u8 = 0x09;
pub const CURSOR_START: u8 = 0x0a;
pub const CURSOR_END: u8 = 0x0b;
pub const START_ADDRESS_HIGH: u8 = 0x0c;
pub const START_ADDRESS_LOW: u8 = 0x0d;
pub const CURSOR_ADDRESS_HIGH: u8 = 0x0e;
pub const CURSOR_ADDRESS_LOW: u8 = 0x0f;
pub const VERTICAL_RETRACE_START: u8 = 0x10;
pub const VERTICAL_RETRACE_END: u8 = 0x11;
pub const VERTICAL_DISPLAY_END: u8 = 0x12;
pub const OFFSET: u8 = 0x13;
pub const UNDERLINE_LOCATION: u8 = 0x14;
pub const VERTICAL_BLANK_START: u8 = 0x15;
pub const VERTICAL_BLANK_END: u8 = 0x16;
pub const MODE_CONTROL: u8 = 0x17;
pub const LINE_COMPARE: u8 = 0x18;

pub const PROTECT_TIMING_REGISTERS: u8 = 1 << 7;
const LINE_COMPARE_OVERFLOW: u8 = 1 << 4;

/// Programmed CRTC register values and their decoded display fields.
/// CPU port selection and the display-start latch belong to the live CRTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrtcRegisters {
    pub values: [u8; 0x19],
}

impl Default for CrtcRegisters {
    fn default() -> Self {
        let mut values = [0; 0x19];
        for &(index, value) in text_mode::CRTC_REGISTERS {
            values[usize::from(index)] = value;
        }
        Self { values }
    }
}

/// Live CRTC registers, the CPU register index and the display-start latch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    index: u8,
    pub registers: CrtcRegisters,
    /// Start address selected at the last vertical retrace. CPU reads of 0C/0D
    /// still return the programmed bytes, which can differ until the next edge.
    pub display_start: u16,
}

impl State {
    pub(crate) fn latch_display_start(&mut self) {
        self.display_start = u16::from_be_bytes([
            self.registers.register(START_ADDRESS_HIGH),
            self.registers.register(START_ADDRESS_LOW),
        ]);
    }

    pub fn index(&self) -> u8 {
        self.index
    }

    pub(crate) fn select(&mut self, index: u8) -> bool {
        let index = index & 0x1f;
        let changed = self.index != index;
        self.index = index;
        changed
    }

    pub(crate) fn read(&self) -> u8 {
        match self.index {
            HORIZONTAL_TOTAL..=LINE_COMPARE => self.registers.register(self.index),
            _ => 0xff,
        }
    }

    pub(crate) fn write(&mut self, mut value: u8) {
        let index = self.index;
        if index <= OVERFLOW
            && self.registers.register(VERTICAL_RETRACE_END) & PROTECT_TIMING_REGISTERS != 0
        {
            // CRTC 11 bit 7 protects 00–07 except the line-compare overflow bit.
            if index == OVERFLOW {
                self.registers.values[usize::from(OVERFLOW)] = (self.registers.register(OVERFLOW)
                    & !LINE_COMPARE_OVERFLOW)
                    | (value & LINE_COMPARE_OVERFLOW);
            }
            return;
        }
        match index {
            CURSOR_START => value &= 0x3f, // Scanline and cursor-disable bits.
            CURSOR_END => value &= 0x7f,   // Scanline and cursor-skew bits.
            PRESET_ROW_SCAN | UNDERLINE_LOCATION => value &= 0x7f,
            HORIZONTAL_TOTAL..=LINE_COMPARE => {}
            _ => return,
        }
        self.registers.values[usize::from(index)] = value;
    }
}

impl CrtcRegisters {
    fn register(&self, index: u8) -> u8 {
        self.values[usize::from(index)]
    }

    /// CRTC 00 stores the total character clocks per scanline minus five.
    pub(crate) fn horizontal_total(&self) -> u64 {
        u64::from(self.register(HORIZONTAL_TOTAL)) + 5
    }

    /// CRTC 01 stores the last displayed character clock, inclusive.
    pub(crate) fn horizontal_display(&self) -> u64 {
        u64::from(self.register(HORIZONTAL_DISPLAY_END)) + 1
    }

    pub(crate) fn horizontal_display_skew(&self) -> u64 {
        // CRTC 03 bits 6:5 delay display enable by zero to three character clocks.
        u64::from((self.register(HORIZONTAL_BLANK_END) >> 5) & 3)
    }

    /// CRTC 06 plus Overflow bits 0/5 form a ten-bit total-minus-two counter.
    pub(crate) fn vertical_total(&self) -> u64 {
        let overflow = self.register(OVERFLOW);
        u64::from(self.register(VERTICAL_TOTAL))
            + (u64::from(overflow & 0x01) << 8)
            + (u64::from(overflow & 0x20) << 4)
            + 2
    }

    /// CRTC 12 plus Overflow bits 1/6 name the last visible vertical count.
    pub(crate) fn vertical_display(&self) -> u64 {
        let overflow = self.register(OVERFLOW);
        u64::from(self.register(VERTICAL_DISPLAY_END))
            + (u64::from(overflow & 0x02) << 7)
            + (u64::from(overflow & 0x40) << 3)
            + 1
    }

    /// Blank output intervals in vertical-counter units, after the one-count
    /// output delay. The second interval covers blanking across counter reset.
    /// Original VGA compares seven end bits; see the compatibility README for
    /// the edge convention and its limits. The shader uses the same convention.
    pub(crate) fn vertical_blanking(&self) -> [Range<u32>; 2] {
        let start = u32::from(self.register(VERTICAL_BLANK_START))
            | (u32::from(self.register(OVERFLOW) & 8) << 5)
            | (u32::from(self.register(MAXIMUM_SCAN_LINE) & 0x20) << 4);
        let total = self.vertical_total() as u32;
        if start >= total {
            return [0..0, 0..0];
        }
        let end_low = u32::from(self.register(VERTICAL_BLANK_END) & 0x7f);
        let end = start + ((end_low.wrapping_sub(start + 1) & 0x7f) + 1) + 1;
        let head_end = if end > total {
            (end_low + 1).min(total)
        } else {
            0
        };
        // An unreachable end, or coincident comparators with set priority,
        // leaves blanking active throughout the steady-state frame.
        if head_end > start {
            [0..total, 0..0]
        } else {
            [start + 1..end.min(total), 0..head_end]
        }
    }

    /// CRTC 10 plus Overflow bits 2/7 form the ten-bit retrace start count.
    pub(crate) fn vertical_retrace_start(&self) -> u64 {
        let overflow = self.register(OVERFLOW);
        u64::from(self.register(VERTICAL_RETRACE_START))
            | (u64::from(overflow & 0x04) << 6)
            | (u64::from(overflow & 0x80) << 2)
    }

    /// Retrace ends when the vertical counter's low four bits match CRTC 11.
    pub(crate) fn vertical_retrace_end_low_bits(&self) -> u64 {
        u64::from(self.register(VERTICAL_RETRACE_END) & 0x0f)
    }

    pub(crate) fn scanlines_per_vertical_count(&self) -> u64 {
        // CRTC 17 bit 2 divides the vertical-counter clock by two. Character
        // height and double scanning do not multiply these physical totals.
        if self.register(MODE_CONTROL) & (1 << 2) == 0 {
            1
        } else {
            2
        }
    }

    pub(crate) fn sync_enabled(&self) -> bool {
        self.register(MODE_CONTROL) & (1 << 7) != 0
    }
}
