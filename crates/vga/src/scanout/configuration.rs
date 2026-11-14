//! Prepared display interpretation, resolved with live latches at capture.
use super::{Scanout, VgaDisplayLayout, VgaDisplayState, VgaParameters};
use crate::{snapshot::Blanking, PackedLayout, Registers};
use std::sync::Arc;

pub(crate) struct DisplayConfiguration {
    pub scanout: PreparedScanout,
    pub blanked: bool,
    pub blanking: Blanking,
}

#[derive(Clone)]
pub(crate) enum PreparedScanout {
    Vga {
        layout: VgaDisplayLayout,
        parameters: Arc<VgaParameters>,
    },
    /// A complete viewport, including its programmed physical start address.
    Packed(PackedLayout),
    /// A wrapping viewport whose start includes panning but not the CRTC latch.
    PackedWithCrtcStart {
        layout: PackedLayout,
        bytes_per_address: u32,
    },
}

impl DisplayConfiguration {
    pub fn vga(registers: &Registers) -> Self {
        Self {
            scanout: PreparedScanout::vga(registers, VgaDisplayLayout::vga(registers)),
            blanked: false,
            blanking: Blanking::default(),
        }
    }

    pub fn packed_layout(&self) -> Option<PackedLayout> {
        match &self.scanout {
            PreparedScanout::Vga { .. } => None,
            PreparedScanout::Packed(layout)
            | PreparedScanout::PackedWithCrtcStart { layout, .. } => Some(*layout),
        }
    }

    /// Resolve a CRTC counter value into this capture's display interpretation.
    /// Native drawing supplies the programmed counter instead of its latch.
    pub fn scanout_at(&self, registers: &Registers, start: u32, retrace_count: u64) -> Scanout {
        match &self.scanout {
            PreparedScanout::Vga { layout, parameters } => {
                // Raw register inspection includes CPU-only controls. Copying
                // them here keeps those writes independent of display preparation.
                let state =
                    VgaDisplayState::from_registers(registers, *layout, parameters.clone(), start);
                Scanout::Vga(state.with_blink_phase(retrace_count))
            }
            PreparedScanout::Packed(layout) => Scanout::Packed(*layout),
            PreparedScanout::PackedWithCrtcStart {
                layout,
                bytes_per_address,
            } => Scanout::Packed(layout.with_start_offset(start * *bytes_per_address)),
        }
    }
}

impl PreparedScanout {
    pub fn vga(registers: &Registers, layout: VgaDisplayLayout) -> Self {
        Self::Vga {
            parameters: Arc::new(VgaParameters::new(registers, &layout)),
            layout,
        }
    }
}
