use crate::{
    registers::{
        attribute::{self, Attribute},
        crtc::{self, Crtc},
        graphics::Graphics,
        sequencer::Sequencer,
        setup::Setup,
    },
    Vga,
};

/// Standard VGA registers, CPU access phases/latches and adapter address decoding.
/// Palette colors and Color Select belong to `DacState`; DISPI programming belongs
/// to `DispiState`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VgaCoreState {
    crtc: Crtc,
    sequencer: Sequencer,
    graphics: Graphics,
    attribute: Attribute,
    miscellaneous: u8,
    feature_control: u8,
    setup: Setup,
    latches: [u8; 4],
}

impl VgaCoreState {
    pub const BYTE_LEN: usize = 74;

    pub(super) fn capture(vga: &Vga) -> Self {
        let registers = &vga.registers;
        let mut attribute = registers.attribute.clone();
        attribute.values[attribute::COLOR_SELECT as usize] = 0;
        Self {
            crtc: registers.crtc.clone(),
            sequencer: registers.sequencer.clone(),
            graphics: registers.graphics.clone(),
            attribute,
            miscellaneous: registers.miscellaneous,
            feature_control: registers.feature_control,
            setup: vga.setup.clone(),
            latches: vga.memory.latches(),
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        let [control, option] = self.setup.to_bytes();
        bytes[..9].copy_from_slice(&[
            self.miscellaneous,
            self.feature_control,
            control,
            option,
            self.sequencer.index(),
            self.graphics.index(),
            self.crtc.index(),
            self.attribute.address(),
            u8::from(self.attribute.data_next()),
        ]);
        bytes[9..11].copy_from_slice(&self.crtc.display_start.to_le_bytes());
        bytes[11..15].copy_from_slice(&self.latches);
        bytes[15..20].copy_from_slice(&self.sequencer.values);
        bytes[20..29].copy_from_slice(&self.graphics.values);
        bytes[29..54].copy_from_slice(&self.crtc.registers.values);
        bytes[54..].copy_from_slice(&self.attribute.values[..20]);
        bytes
    }

    pub fn from_bytes(bytes: &[u8; Self::BYTE_LEN]) -> Option<Self> {
        // Reuse each bank's write masks; a noncanonical encoding is rejected.
        let mut sequencer = Sequencer::default();
        for (index, &value) in bytes[15..20].iter().enumerate() {
            sequencer.select(index as u8);
            sequencer.write(value);
        }
        sequencer.select(bytes[4]);
        let mut graphics = Graphics::default();
        for (index, &value) in bytes[20..29].iter().enumerate() {
            graphics.select(index as u8);
            graphics.write(value);
        }
        graphics.select(bytes[5]);
        let mut crtc = Crtc::default();
        crtc.registers.values[crtc::VERTICAL_RETRACE_END as usize] &=
            !crtc::PROTECT_TIMING_REGISTERS;
        for (index, &value) in bytes[29..54].iter().enumerate() {
            crtc.select(index as u8);
            crtc.write(value);
        }
        crtc.select(bytes[6]);
        crtc.display_start = u16::from_le_bytes(bytes[9..11].try_into().unwrap());
        let mut attribute = Attribute::default();
        for (index, &value) in bytes[54..].iter().enumerate() {
            attribute.reset_address();
            attribute.write(index as u8);
            attribute.write(value);
        }
        attribute.reset_address();
        attribute.write(bytes[7]);
        if bytes[8] == 0 {
            attribute.reset_address();
        }
        attribute.values[attribute::COLOR_SELECT as usize] = 0;
        let state = Self {
            crtc,
            sequencer,
            graphics,
            attribute,
            miscellaneous: bytes[0],
            feature_control: bytes[1],
            setup: Setup::from_bytes([bytes[2], bytes[3]])?,
            latches: bytes[11..15].try_into().unwrap(),
        };
        (state.to_bytes() == *bytes).then_some(state)
    }

    pub(super) fn restore(self, vga: &mut Vga) {
        let color_select = vga.registers.attribute.values[attribute::COLOR_SELECT as usize];
        vga.registers.crtc = self.crtc;
        vga.registers.sequencer = self.sequencer;
        vga.registers.graphics = self.graphics;
        vga.registers.attribute = self.attribute;
        vga.registers.attribute.values[attribute::COLOR_SELECT as usize] = color_select;
        vga.registers.miscellaneous = self.miscellaneous;
        vga.registers.feature_control = self.feature_control;
        vga.setup = self.setup;
        vga.memory.restore_latches(self.latches);
    }
}
