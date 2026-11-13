use nuked_opl3::Opl3Chip;

/// Nine-channel mono synthesis through Nuked-OPL3's OPL2 compatibility mode.
/// Timers and guest ports belong to the AdLib device, not this sample generator.
pub struct Opl2 {
    chip: Opl3Chip,
    waveforms: [u8; 22],
    waveforms_enabled: bool,
}

impl Opl2 {
    /// Creates a silent chip at an output rate in 8,000..=192,000 Hz.
    /// This range also keeps the core's fixed-point resampler arithmetic in range.
    pub fn new(sample_rate: u32) -> Result<Self, String> {
        if !(8_000..=192_000).contains(&sample_rate) {
            return Err("AdLib output needs a sample rate in 8000..=192000 Hz".into());
        }
        Ok(Self {
            chip: Opl3Chip::new(sample_rate),
            waveforms: [0; 22],
            waveforms_enabled: false,
        })
    }

    /// Writes the OPL2 register bank. The second OPL3 bank is not exposed.
    pub fn write_register(&mut self, register: u8, value: u8) {
        match register {
            // YMF262 compatibility mode lacks the YM3812 waveform-enable bit.
            0x01 => {
                self.waveforms_enabled = value & 0x20 != 0;
                for offset in 0..self.waveforms.len() {
                    self.write_waveform(offset);
                }
            }
            0xe0..=0xf5 => {
                let offset = usize::from(register - 0xe0);
                self.waveforms[offset] = value & 3;
                self.write_waveform(offset);
            }
            _ => self.chip.write_register(u16::from(register), value),
        }
    }

    fn write_waveform(&mut self, offset: usize) {
        let waveform = if self.waveforms_enabled {
            self.waveforms[offset]
        } else {
            0
        };
        self.chip.write_register(0xe0 + offset as u16, waveform);
    }

    /// Generates one mono sample in -1..=1 using the core's resampler.
    /// The embedding chooses gain and mixes it with other devices.
    pub fn sample(&mut self) -> f32 {
        let mut stereo = [0; 2];
        self.chip
            .generate_resampled(&mut stereo)
            .expect("two-channel sample buffer");
        f32::from(stereo[0]) / 32768.0
    }
}
