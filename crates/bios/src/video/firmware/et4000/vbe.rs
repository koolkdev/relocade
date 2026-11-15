//! Card implementation of the shared VBE firmware contract.
mod modes;
use super::modes::Mode;
use crate::video::firmware::vbe::{ModeInformation, SavedState, Vbe, WindowAccess};
use relocade_machine::{IoBus, IoContext, MemoryView};
use relocade_vga::{
    registers::dac::Dac, DacState, Et4000Ax, Et4000State, PackedLayout, RetraceEvent, VgaCoreState,
    Viewport,
};
use std::{cell::RefCell, future::Future};

impl Vbe for Et4000Ax {
    const OEM_STRING: &'static [u8] = b"Relocade ET4000AX\0";
    const DAC_BITS_PER_COMPONENT: u8 = Dac::BITS_PER_COMPONENT;
    // EV8913 state version 2 includes the SC11483 command/access state.
    const STATE_PROFILE: u8 = 2;
    const EXTENSION_STATE_BYTES: usize = Et4000State::BYTE_LEN;

    fn mode_numbers() -> impl Iterator<Item = u16> {
        Mode::ALL.into_iter().map(|mode| mode.vbe_number)
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
        let mode = Mode::from_vbe_number(number).filter(|_| self.ports_enabled())?;
        let layout = mode.layout(self.video_memory_bytes())?;
        let layout = self
            .packed_layout()
            .filter(|active| active.size() == layout.size() && active.format() == layout.format())
            .unwrap_or(layout);
        Some(ModeInformation {
            layout,
            windows: [WindowAccess::Write, WindowAccess::Read],
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
            (0, Some(bank)) => self.set_write_bank(context, bank).then_some(bank),
            (1, Some(bank)) => self.set_read_bank(context, bank).then_some(bank),
            (0, None) => Some(self.write_bank()),
            (1, None) => Some(self.read_bank()),
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

    fn capture_state(&self) -> SavedState {
        SavedState {
            core: self.core_state().to_bytes(),
            dac: self.dac_state().to_bytes(),
            extensions: self.et4000_state().to_bytes().to_vec(),
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
            Some(bytes) => Some(Self::decode_core_state(bytes)?),
            None => None,
        };
        let dac = match dac {
            Some(bytes) => Some(DacState::from_bytes(bytes)?),
            None => None,
        };
        let extensions = match extensions {
            Some(bytes) => Some(Et4000State::from_bytes(bytes.try_into().ok()?)?),
            None => None,
        };
        self.restore_state(context, core, dac, extensions);
        Some(())
    }
}
