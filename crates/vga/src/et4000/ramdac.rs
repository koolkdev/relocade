//! SC11483 command access and pixel interpretation around the shared VGA palette.
//! Sierra SC11482/SC11483/SC11484 data sheet, pp. 5–6:
//! https://www.dosdays.co.uk/media/sierra_semi/SC11482CV.pdf
use crate::{effects::Effects, registers::dac::Dac, PixelFormat};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Sc11483 {
    command: u8,
    mask_reads: u8,
}

impl Sc11483 {
    pub fn read_mask(&mut self, palette: &Dac) -> u8 {
        if self.mask_reads == 4 {
            self.command
        } else {
            self.mask_reads += 1;
            palette.mask
        }
    }

    pub fn write_effect(&self) -> Effects {
        if self.mask_reads == 4 {
            Effects::CONFIGURATION
        } else {
            Effects::IMAGE
        }
    }

    pub fn write_mask(&mut self, palette: &mut Dac, value: u8) {
        if self.mask_reads == 4 {
            self.write_command(value);
        } else {
            palette.mask = value;
        }
        // Any write ends the consecutive-read sequence, including a command write.
        self.mask_reads = 0;
    }

    pub fn write_command(&mut self, value: u8) {
        self.command = value;
        self.mask_reads = 0;
    }

    /// Access to any other DAC register ends command access without changing
    /// the selected pixel format or the palette's transfer position.
    pub fn access_palette<'a>(&mut self, palette: &'a mut Dac) -> &'a mut Dac {
        self.mask_reads = 0;
        palette
    }

    pub fn format(&self) -> PixelFormat {
        if self.command & 0x80 == 0 {
            PixelFormat::Indexed8
        } else {
            PixelFormat::Rgb555
        }
    }

    /// Mode 1 latches both clock edges; mode 2 latches consecutive rising edges.
    pub fn clocks_per_pixel(&self) -> u32 {
        if self.command & 0xa0 == 0xa0 {
            2
        } else {
            1
        }
    }

    pub fn to_state_bytes(&self) -> [u8; 2] {
        [self.command, self.mask_reads]
    }

    pub fn from_state_bytes([command, mask_reads]: [u8; 2]) -> Option<Self> {
        (mask_reads <= 4).then_some(Self {
            command,
            mask_reads,
        })
    }
}
