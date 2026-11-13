//! Effects of VGA state changes on display output and address decoding.

mod writes;

use crate::{registers::WriteKind, DispiAdapter, PackedLayout, Timing};
use relocade_machine::IoContext;

#[derive(Clone, Copy)]
pub(super) enum VideoEffect {
    None,
    Image,
    Configuration,
}

#[derive(Clone, Copy)]
pub(super) struct Effects {
    video: VideoEffect,
    port_decode: bool,
    memory_decode: bool,
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

impl DispiAdapter {
    pub(super) fn legacy_effect(&self, kind: WriteKind) -> Effects {
        if self.dispi.display_layout().is_some() {
            return Effects::NONE;
        }
        match kind {
            WriteKind::Access => Effects::NONE,
            WriteKind::Display => Effects::IMAGE,
            WriteKind::Timing => Effects::CONFIGURATION,
        }
    }

    /// Preserve overdue output, apply the state change, then update video
    /// configuration and address decoding before returning. Preparation can update
    /// hardware latches, so the mutation must use the current live register bank.
    /// Register-only and decoding-only operations do no clock or video work.
    pub(super) fn with_effects<R>(
        &mut self,
        mut context: IoContext<'_>,
        effects: Effects,
        mutate: impl FnOnce(&mut Self) -> R,
    ) -> R {
        match effects.video {
            VideoEffect::None => {}
            VideoEffect::Image => self.capture_if_due(),
            VideoEffect::Configuration => self.advance_raster(self.clock.now()),
        }
        let previous_layout = matches!(effects.video, VideoEffect::Configuration)
            .then(|| self.dispi.display_layout());
        let previous_decoding = effects.port_decode.then(|| self.decoding());
        let previous_memory = effects.memory_decode.then(|| self.memory_decoding());
        let result = mutate(self);
        let video_changed =
            previous_layout.is_some_and(|previous| self.update_video_configuration(previous));
        let decoding_changed = previous_decoding.is_some_and(|previous| {
            let decoding = self.decoding();
            if previous == decoding {
                return false;
            }
            if let Some(io) = &self.io {
                io.ports
                    .update_decoding(context.reborrow(), Some(previous), decoding);
            }
            true
        });
        if let Some(previous) = previous_memory {
            let decoding = self.memory_decoding();
            if previous != decoding {
                if let super::device::MemoryDecoding::Packed(window) = decoding {
                    self.packed_window = window;
                }
                if let Some(io) = &self.io {
                    io.memory
                        .update_decoding(context.reborrow(), Some(previous), decoding);
                }
            }
        }
        if video_changed || decoding_changed {
            self.configuration_changed.notify(usize::MAX);
        }
        result
    }

    fn update_video_configuration(&mut self, previous_layout: Option<PackedLayout>) -> bool {
        let layout = self.dispi.display_layout();
        let timing = match layout {
            Some(_) => self.dispi.timing().expect("active DISPI timing"),
            None => Timing::vga(&self.vga.registers),
        };
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
