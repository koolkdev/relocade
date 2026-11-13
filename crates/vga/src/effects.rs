//! Preparation and completion around video state changes.
use crate::{output::Output, registers::WriteKind, PackedLayout, Timing};
use relocade_machine::Timestamp;

#[derive(Clone, Copy)]
pub(crate) enum VideoEffect {
    None,
    Image,
    Configuration,
}

#[derive(Clone, Copy)]
pub(crate) struct Effects {
    pub video: VideoEffect,
    pub port_decode: bool,
    pub memory_decode: bool,
}

impl Effects {
    pub const NONE: Self = Self {
        video: VideoEffect::None,
        port_decode: false,
        memory_decode: false,
    };
    pub const IMAGE: Self = Self {
        video: VideoEffect::Image,
        ..Self::NONE
    };
    pub const CONFIGURATION: Self = Self {
        video: VideoEffect::Configuration,
        ..Self::NONE
    };
    pub const IO: Self = Self {
        port_decode: true,
        ..Self::MEMORY
    };
    pub const CONFIGURATION_AND_IO: Self = Self {
        video: VideoEffect::Configuration,
        ..Self::IO
    };
    pub const MEMORY: Self = Self {
        memory_decode: true,
        ..Self::NONE
    };
    pub const CONFIGURATION_AND_MEMORY: Self = Self {
        video: VideoEffect::Configuration,
        ..Self::MEMORY
    };
}

impl Effects {
    pub fn vga(kind: WriteKind) -> Self {
        match kind {
            WriteKind::Access => Self::NONE,
            WriteKind::Display => Self::IMAGE,
            WriteKind::Timing => Self::CONFIGURATION,
        }
    }
}

impl Output {
    /// Return the time to service before mutation. The card applies its own
    /// retrace-latch and scanout policy; access-only changes never read the clock.
    pub fn prepare(&self, effect: VideoEffect) -> Option<Timestamp> {
        match effect {
            VideoEffect::None => None,
            VideoEffect::Image => self.capture_due(),
            VideoEffect::Configuration => Some(self.clock.now()),
        }
    }

    /// Update timing and layout after mutation. A change of packed dimensions or
    /// entry/exit restarts the beam; other changes retain its accumulated phase.
    pub fn update_configuration(
        &mut self,
        previous_layout: Option<PackedLayout>,
        layout: Option<PackedLayout>,
        timing: Timing,
    ) -> bool {
        let timing_changed =
            if layout.map(|layout| layout.size()) != previous_layout.map(|layout| layout.size()) {
                self.raster.restart(timing);
                true
            } else {
                self.raster.set_timing(timing)
            };
        timing_changed || layout != previous_layout
    }
}
