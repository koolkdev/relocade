//! Effects of video state changes on display output and address decoding.
use crate::{
    io::Io, mmio::MemoryDecoding, output::Output, ports::PortDecoding, registers::WriteKind,
    PackedLayout, Timing,
};
use relocade_machine::{IoContext, Timestamp};

#[derive(Clone, Copy)]
pub(crate) enum VideoEffect {
    None,
    Image,
    Configuration,
}

#[derive(Clone, Copy)]
pub(crate) struct Effects {
    pub video: VideoEffect,
    pub scanout: bool,
    pub port_decode: bool,
    pub memory_decode: bool,
}

impl Effects {
    pub const NONE: Self = Self {
        video: VideoEffect::None,
        scanout: false,
        port_decode: false,
        memory_decode: false,
    };
    pub const IMAGE: Self = Self {
        video: VideoEffect::Image,
        ..Self::NONE
    };
    pub const CONFIGURATION: Self = Self {
        video: VideoEffect::Configuration,
        scanout: true,
        ..Self::NONE
    };
    pub const IO: Self = Self {
        port_decode: true,
        ..Self::MEMORY
    };
    pub const CONFIGURATION_AND_IO: Self = Self {
        video: VideoEffect::Configuration,
        scanout: true,
        ..Self::IO
    };
    pub const MEMORY: Self = Self {
        memory_decode: true,
        ..Self::NONE
    };
    pub const CONFIGURATION_AND_MEMORY: Self = Self {
        video: VideoEffect::Configuration,
        scanout: true,
        ..Self::MEMORY
    };
}

impl Effects {
    pub fn vga(kind: WriteKind) -> Self {
        let mut effects = match kind {
            WriteKind::Access => Self::NONE,
            WriteKind::Display => Self::IMAGE,
            WriteKind::Timing => Self::CONFIGURATION,
        };
        // Keep raw VGA inspection current even for CPU-only control writes.
        effects.scanout = true;
        effects
    }
}

/// Card policy used by the shared synchronous effects operation. Calls are
/// specialized for the concrete card; installed I/O operations use no vtable.
pub(crate) trait CardEffects: Sized {
    fn output(&mut self) -> &mut Output;
    fn io(&self) -> Option<&Io>;
    fn advance_raster(&mut self, now: Timestamp);
    fn timing(&self) -> Timing;
    fn port_decoding(&self) -> PortDecoding;
    fn memory_decoding(&self) -> MemoryDecoding;
    fn update_scanout(&mut self, effect: VideoEffect);

    fn display_layout(&self) -> Option<PackedLayout> {
        None
    }

    /// Preserve overdue output, mutate live state, then apply configuration and
    /// decoding changes synchronously. Preparation can update hardware latches;
    /// remember only the previous layout and routing keys after that work.
    fn with_effects<R>(
        &mut self,
        mut context: IoContext<'_>,
        effects: Effects,
        mutate: impl FnOnce(&mut Self) -> R,
    ) -> R {
        if let Some(now) = self.output().prepare(effects.video) {
            self.advance_raster(now);
        }
        let previous_layout =
            matches!(effects.video, VideoEffect::Configuration).then(|| self.display_layout());
        let previous_ports = effects.port_decode.then(|| self.port_decoding());
        let previous_memory = effects.memory_decode.then(|| self.memory_decoding());

        let result = mutate(self);

        if effects.scanout {
            self.update_scanout(effects.video);
        }

        let video_changed = previous_layout.is_some_and(|previous| {
            let layout = self.display_layout();
            let timing = self.timing();
            self.output().update_configuration(previous, layout, timing)
        });
        let ports_changed = previous_ports.is_some_and(|previous| {
            let decoding = self.port_decoding();
            if previous == decoding {
                return false;
            }
            if let Some(io) = self.io() {
                io.ports
                    .update_decoding(context.reborrow(), Some(previous), decoding);
            }
            true
        });
        if let Some(previous) = previous_memory {
            let decoding = self.memory_decoding();
            if previous != decoding {
                if let Some(io) = self.io() {
                    io.memory
                        .update_decoding(context.reborrow(), Some(previous), decoding);
                }
            }
        }
        if video_changed || ports_changed {
            self.output().configuration_changed.notify(usize::MAX);
        }
        result
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
