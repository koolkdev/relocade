//! Services shared by the selected card's VBE firmware.
mod information;
mod layout;
mod modes;
mod palette;
mod state;

use super::VideoFirmware;
pub(super) use information::{initialize, ModeInformation, WindowAccess};
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, IoContext,
};
use relocade_vga::{LegacyLayout, PackedLayout, RetraceEvent};

#[interrupt(context(call), args(function = al))]
pub(in crate::video) async fn dispatch(
    call: &mut CpuContext<'_>,
    firmware: &VideoFirmware,
    function: u8,
) {
    interrupt_dispatch!(
        call,
        match function {
            _ if matches!(firmware, VideoFirmware::Vga) => unsupported,
            0x00 => information::controller(firmware),
            0x01 => information::mode(firmware),
            0x02 => modes::set(firmware),
            0x03 => modes::current(firmware),
            0x04 => state::dispatch(firmware),
            0x05 => modes::window(firmware),
            0x06 => layout::scanline(firmware),
            0x07 => layout::display_start(firmware),
            0x08 => palette::format(firmware),
            _ => unsupported,
        }
    );
}

// VBE reports status in AX and preserves flags, including carry.
#[interrupt(returns { () => { ax = 0x0100 } })]
fn unsupported() {}

impl VideoFirmware {
    fn ports_enabled(&self) -> bool {
        match self {
            Self::Vga => false,
            Self::Dispi(card) => card.borrow().ports_enabled(),
            Self::Et4000Ax(card) => card.borrow().ports_enabled(),
        }
    }

    fn packed_layout(&self) -> Option<PackedLayout> {
        if !self.ports_enabled() {
            return None;
        }
        match self {
            Self::Vga => None,
            Self::Dispi(card) => card.borrow().packed_layout(),
            Self::Et4000Ax(card) => card.borrow().packed_layout(),
        }
    }

    fn video_memory_bytes(&self) -> Option<u32> {
        match self {
            Self::Vga => None,
            Self::Dispi(card) => Some(card.borrow().video_memory_bytes()),
            Self::Et4000Ax(card) => Some(card.borrow().video_memory_bytes()),
        }
    }

    fn legacy_layout(&self) -> Option<LegacyLayout> {
        if !self.ports_enabled() || self.packed_layout().is_some() {
            return None;
        }
        match self {
            Self::Vga => None,
            Self::Dispi(card) => LegacyLayout::from_registers(card.borrow().registers()),
            Self::Et4000Ax(card) => LegacyLayout::from_registers(card.borrow().registers()),
        }
    }

    // VBE requests bounded viewports; native hardware may also wrap its counter.
    fn bounded_layout(&self, pitch: u32) -> Option<PackedLayout> {
        let layout = self.packed_layout()?;
        let capacity = self.video_memory_bytes()?;
        PackedLayout::new(layout.size(), layout.format(), pitch, 0, capacity)
    }

    fn max_packed_pitch(&self) -> Option<u32> {
        match self {
            Self::Et4000Ax(card) => card.borrow().max_packed_pitch(),
            _ => Some(self.packed_layout()?.max_pitch()),
        }
    }

    fn set_packed_pitch(&self, context: IoContext<'_>, pitch: u32) -> Option<PackedLayout> {
        match self {
            Self::Vga => None,
            Self::Dispi(card) => card.borrow_mut().set_packed_pitch(context, pitch),
            Self::Et4000Ax(card) => card.borrow_mut().set_packed_pitch(context, pitch),
        }
    }

    fn set_display_origin(&self, context: IoContext<'_>, origin: [u16; 2]) -> bool {
        match self {
            Self::Vga => false,
            Self::Dispi(card) => card.borrow_mut().set_display_origin(context, origin),
            Self::Et4000Ax(card) => card.borrow_mut().set_display_origin(context, origin),
        }
    }

    async fn wait_for_retrace(&self) -> Option<RetraceEvent> {
        match self {
            Self::Vga => None,
            Self::Dispi(card) => {
                let wait = card.borrow_mut().wait_for_retrace()?;
                Some(wait.await)
            }
            Self::Et4000Ax(card) => {
                let wait = card.borrow_mut().wait_for_retrace()?;
                Some(wait.await)
            }
        }
    }
}
