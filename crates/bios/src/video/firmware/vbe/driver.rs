//! Card operations consumed by the shared VBE register and buffer ABI.
use super::ModeInformation;
use relocade_machine::{IoBus, IoContext, MemoryView};
use relocade_vga::{DacState, PackedLayout, RetraceEvent, VgaCoreState, Viewport};
use std::{cell::RefCell, future::Future};

/// Owned device bytes let firmware release the card before writing guest memory,
/// which may itself be mapped to the card's VRAM.
pub(in crate::video::firmware) struct SavedState {
    pub core: [u8; VgaCoreState::BYTE_LEN],
    pub dac: [u8; DacState::BYTE_LEN],
    pub extensions: Vec<u8>,
}

pub(in crate::video::firmware) trait Vbe: Sized {
    const OEM_STRING: &'static [u8];
    const DAC_BITS_PER_COMPONENT: u8;
    /// Identifies the card and encoding version in the opaque VBE state buffer.
    const STATE_PROFILE: u8;
    const EXTENSION_STATE_BYTES: usize;

    fn mode_numbers() -> impl Iterator<Item = u16>;
    fn ports_enabled(&self) -> bool;
    fn video_memory_bytes(&self) -> u32;
    fn packed_layout(&self) -> Option<PackedLayout>;
    fn mode_information(&self, number: u16) -> Option<ModeInformation>;

    /// Mode entry also uses shared VGA port programming and BIOS bookkeeping.
    /// Borrow the card only around direct operations so those port accesses can
    /// reenter its connected device callbacks.
    fn set_mode(
        card: &RefCell<Self>,
        memory: &mut MemoryView<'_>,
        io: &IoBus,
        number: u16,
        preserve: bool,
    ) -> bool;
    fn current_mode(&self, memory: &MemoryView<'_>) -> Option<u16>;
    /// None queries the selected window; Some selects its bank.
    fn window(&mut self, context: IoContext<'_>, window: u8, bank: Option<u16>) -> Option<u16>;

    fn viewport(&self) -> Option<Viewport>;
    fn set_display_pitch(&mut self, context: IoContext<'_>, pitch: u32) -> Option<Viewport>;
    fn set_display_origin(&mut self, context: IoContext<'_>, origin: [u16; 2]) -> bool;
    /// The owned wait must not retain a card borrow across suspension.
    fn wait_for_retrace(&mut self) -> Option<impl Future<Output = RetraceEvent> + 'static>;

    fn capture_state(&self) -> SavedState;
    /// Validate every requested component before applying any of them together.
    fn restore_state(
        &mut self,
        context: IoContext<'_>,
        core: Option<&[u8; VgaCoreState::BYTE_LEN]>,
        dac: Option<&[u8; DacState::BYTE_LEN]>,
        extensions: Option<&[u8]>,
    ) -> Option<()>;
}
