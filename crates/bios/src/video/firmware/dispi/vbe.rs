//! Card implementation of the shared VBE firmware contract.
mod modes;
use crate::video::firmware::vbe::{ModeInformation, SavedState, Vbe, WindowAccess};
use modes::Mode;
use relocade_machine::{IoBus, IoContext, MemoryView};
use relocade_vga::{
    DacState, DispiAdapter, DispiState, PackedLayout, RetraceEvent, VgaCoreState, Viewport,
};
use std::{cell::RefCell, future::Future};

impl Vbe for DispiAdapter {
    const OEM_STRING: &'static [u8] = b"Relocade banked SVGA\0";
    const MAX_DAC_BITS_PER_COMPONENT: u8 = 8;
    // DISPI state with interface negotiation and capability queries.
    const STATE_PROFILE: u8 = 3;
    const EXTENSION_STATE_BYTES: usize = DispiState::BYTE_LEN;

    fn mode_numbers() -> impl Iterator<Item = u16> {
        Mode::ALL.into_iter().map(|mode| mode.number)
    }

    fn ports_enabled(&self) -> bool {
        self.ports_enabled()
    }
    fn video_memory_bytes(&self) -> u32 {
        self.video_memory_bytes()
    }
    fn packed_layout(&self) -> Option<PackedLayout> {
        self.packed_layout()
    }

    fn mode_information(&self, number: u16) -> Option<ModeInformation> {
        let mode = Mode::from_number(number).filter(|_| self.ports_enabled())?;
        let layout = self
            .packed_layout()
            .filter(|layout| {
                layout.size() == mode.display.size() && layout.format() == mode.display.format()
            })
            .unwrap_or_else(|| {
                mode.display
                    .layout(self.video_memory_bytes())
                    .expect("supported mode fits VRAM")
            });
        Some(ModeInformation {
            layout,
            windows: [WindowAccess::ReadWrite, WindowAccess::Absent],
        })
    }

    fn set_mode(
        card: &RefCell<Self>,
        memory: &mut MemoryView<'_>,
        io: &IoBus,
        number: u16,
        preserve: bool,
    ) -> bool {
        if number < 0x80 {
            return super::set_mode(
                memory,
                io,
                card,
                number as u8 | if preserve { 0x80 } else { 0 },
            )
            .is_some();
        }
        modes::set(memory, io, card, number, preserve)
    }

    fn current_mode(&self, memory: &MemoryView<'_>) -> Option<u16> {
        modes::current(memory, self)
    }

    fn window(&mut self, context: IoContext<'_>, window: u8, bank: Option<u16>) -> Option<u16> {
        match (window, bank) {
            (0, Some(bank)) => self.set_video_bank(context, bank).then_some(bank),
            (0, None) => Some(self.video_bank()),
            _ => None,
        }
    }

    fn viewport(&self) -> Option<Viewport> {
        self.viewport()
    }
    fn set_display_pitch(&mut self, context: IoContext<'_>, pitch: u32) -> Option<Viewport> {
        self.set_display_pitch(context, pitch)
    }
    fn set_display_origin(&mut self, context: IoContext<'_>, origin: [u16; 2]) -> bool {
        self.set_display_origin(context, origin)
    }
    fn wait_for_retrace(&mut self) -> Option<impl Future<Output = RetraceEvent> + 'static> {
        self.wait_for_retrace()
    }

    fn palette(&self) -> &[[u8; 3]; 256] {
        &self.registers().dac.colors
    }
    fn dac_bits_per_component(&self) -> u8 {
        self.dac_bits_per_component()
    }
    fn set_dac_bits_per_component(&mut self, context: IoContext<'_>, bits: u8) {
        self.set_dac_bits_per_component(context, bits);
    }
    fn set_palette(&mut self, context: IoContext<'_>, first: u8, colors: &[[u8; 3]]) {
        self.set_palette(context, first, colors);
    }

    fn capture_state(&self) -> SavedState {
        SavedState {
            core: self.core_state().to_bytes(),
            dac: self.dac_state().to_bytes(),
            extensions: self.dispi_state().to_bytes().to_vec(),
        }
    }

    fn restore_state(
        &mut self,
        context: IoContext<'_>,
        core: Option<&[u8; VgaCoreState::BYTE_LEN]>,
        dac: Option<&[u8; DacState::BYTE_LEN]>,
        extensions: Option<&[u8]>,
    ) -> Option<()> {
        let core = match core {
            Some(bytes) => Some(VgaCoreState::from_bytes(bytes)?),
            None => None,
        };
        let dac = match dac {
            Some(bytes) => Some(DacState::from_bytes(bytes)?),
            None => None,
        };
        let extensions = match extensions {
            Some(bytes) => Some(DispiState::from_bytes(bytes.try_into().ok()?)?),
            None => None,
        };
        self.restore_state(context, core, dac, extensions);
        Some(())
    }
}
