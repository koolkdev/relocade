use crate::{
    registers::{
        attribute::{self, Attribute},
        crtc::{self, Crtc},
        graphics::Graphics,
        sequencer::Sequencer,
        setup::Setup,
    },
    VgaCore,
};

/// Standard VGA registers, CPU access phases/latches and adapter address decoding.
/// Palette colors and Color Select belong to `DacState`; extended card programming is saved separately.
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

    pub(crate) fn capture(vga: &VgaCore) -> Self {
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
            latches: vga.latches,
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        let [control, option] = self.setup.to_bytes();
        bytes[..7].copy_from_slice(&[
            self.miscellaneous,
            self.feature_control,
            control,
            option,
            self.sequencer.index(),
            self.graphics.index(),
            self.crtc.index(),
        ]);
        let attribute = self.attribute.to_state_bytes();
        bytes[7..9].copy_from_slice(&attribute[..2]);
        bytes[9..11].copy_from_slice(&self.crtc.display_start.to_le_bytes());
        bytes[11..15].copy_from_slice(&self.latches);
        bytes[15..20].copy_from_slice(&self.sequencer.values);
        bytes[20..29].copy_from_slice(&self.graphics.values);
        bytes[29..54].copy_from_slice(&self.crtc.registers.values);
        bytes[54..].copy_from_slice(&attribute[2..22]);
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
        // Color Select is encoded separately in DacState.
        let mut attribute = [0; 23];
        attribute[..2].copy_from_slice(&bytes[7..9]);
        attribute[2..22].copy_from_slice(&bytes[54..]);
        let state = Self {
            crtc,
            sequencer,
            graphics,
            attribute: Attribute::from_state_bytes(&attribute)?,
            miscellaneous: bytes[0],
            feature_control: bytes[1],
            setup: Setup::from_bytes([bytes[2], bytes[3]])?,
            latches: bytes[11..15].try_into().unwrap(),
        };
        (state.to_bytes() == *bytes).then_some(state)
    }

    pub(crate) fn restore(self, vga: &mut VgaCore) {
        let color_select = vga.registers.attribute.values[attribute::COLOR_SELECT as usize];
        vga.registers.crtc = self.crtc;
        vga.registers.sequencer = self.sequencer;
        vga.registers.graphics = self.graphics;
        vga.registers.attribute.clone_from(&self.attribute);
        vga.registers.attribute.values[attribute::COLOR_SELECT as usize] = color_select;
        vga.registers.miscellaneous = self.miscellaneous;
        vga.registers.feature_control = self.feature_control;
        vga.setup = self.setup;
        vga.latches = self.latches;
        vga.update_memory_controls();
    }
}
