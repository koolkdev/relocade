//! Attribute Controller register access, IBM VGA Function pages 2-89–2-96.

mod protocol;

use super::text_mode;
use protocol::{AttributeProtocol, WritePhase};

pub const MODE_CONTROL: u8 = 0x10;
pub const OVERSCAN_COLOR: u8 = 0x11;
pub const COLOR_PLANE_ENABLE: u8 = 0x12;
pub const HORIZONTAL_PANNING: u8 = 0x13;
pub const COLOR_SELECT: u8 = 0x14;
pub const PALETTE_ADDRESS_SOURCE: u8 = 0x20;

/// Attribute Controller values and the alternating address/data write protocol.
#[derive(Debug, PartialEq, Eq)]
pub struct Attribute {
    protocol: AttributeProtocol,
    pub values: [u8; 0x15],
}

impl Clone for Attribute {
    fn clone(&self) -> Self {
        Self {
            protocol: self.protocol.clone(),
            values: self.values,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.protocol.clone_from(&source.protocol);
        self.values = source.values;
    }
}

impl Default for Attribute {
    fn default() -> Self {
        let mut values = [0; 0x15];
        values[..16].copy_from_slice(&text_mode::ATTRIBUTE_PALETTE);
        for &(index, value) in text_mode::ATTRIBUTE_REGISTERS {
            values[usize::from(index)] = value;
        }
        Self {
            protocol: AttributeProtocol::new(PALETTE_ADDRESS_SOURCE),
            values,
        }
    }
}

impl Attribute {
    /// Include the Attribute access protocol in an active Input Status 1 read
    /// without borrowing the controller when the prepared reader is invoked.
    pub(crate) fn handle_read(&self, read: impl Fn() -> u8 + 'static) -> impl Fn() -> u8 + 'static {
        self.protocol.handle_read(read)
    }

    pub(crate) fn write_effect(&self) -> super::WriteKind {
        match self.protocol.next_write() {
            WritePhase::Index => super::WriteKind::Display,
            WritePhase::Data if Self::write_mask(self.address()).is_some() => {
                super::WriteKind::Display
            }
            WritePhase::Data => super::WriteKind::Access,
        }
    }

    pub fn address(&self) -> u8 {
        self.protocol.address()
    }

    pub(crate) fn display_enabled(&self) -> bool {
        self.address() & PALETTE_ADDRESS_SOURCE != 0
    }

    fn register(&self, index: u8) -> u8 {
        self.values[usize::from(index)]
    }

    pub(crate) fn read(&self) -> u8 {
        let index = self.address() & 0x1f;
        if index <= COLOR_SELECT {
            self.register(index)
        } else {
            0xff
        }
    }

    pub(crate) fn write(&mut self, value: u8) {
        self.write_with(value, |_, _| false);
    }

    /// A card may consume extension data; standard writes retain VGA masks and
    /// palette gating. There is still one address latch and one access phase.
    pub(crate) fn write_with(&mut self, value: u8, extension: impl FnOnce(u8, u8) -> bool) {
        let values = &mut self.values;
        self.protocol.handle_write(value, |address, value| {
            if !extension(address & 0x1f, value) {
                Self::write_register(values, address, value);
            }
        });
    }

    /// Selected register for a pending data write; index writes have no target.
    pub(crate) fn data_index(&self) -> Option<u8> {
        (self.protocol.next_write() == WritePhase::Data).then(|| self.address() & 0x1f)
    }

    fn write_mask(address: u8) -> Option<u8> {
        Some(match address & 0x1f {
            // IBM Type 1 disables palette access while display addressing is active.
            0..=0x0f if address & PALETTE_ADDRESS_SOURCE == 0 => 0x3f,
            MODE_CONTROL => 0xef,
            OVERSCAN_COLOR => 0xff,
            COLOR_PLANE_ENABLE | HORIZONTAL_PANNING | COLOR_SELECT => 0x0f,
            _ => return None,
        })
    }

    fn write_register(values: &mut [u8; 0x15], address: u8, value: u8) {
        let Some(mask) = Self::write_mask(address) else {
            return;
        };
        values[usize::from(address & 0x1f)] = value & mask;
    }

    pub(crate) fn to_state_bytes(&self) -> [u8; 23] {
        let mut bytes = [0; 23];
        bytes[..2].copy_from_slice(&self.protocol.to_state_bytes());
        bytes[2..].copy_from_slice(&self.values);
        bytes
    }

    pub(crate) fn from_state_bytes(bytes: &[u8; 23]) -> Option<Self> {
        let protocol = AttributeProtocol::from_state_bytes([bytes[0], bytes[1]])?;
        let mut values = [0; 0x15];
        for (index, &value) in bytes[2..].iter().enumerate() {
            Self::write_register(&mut values, index as u8, value);
        }
        let state = Self { protocol, values };
        (state.to_state_bytes() == *bytes).then_some(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn status_reader_resets_live_state_after_restore() {
        let mut attribute = Attribute::default();
        let read_status = attribute.handle_read(|| 0xa5);
        let state = &mut attribute;
        state.write(COLOR_PLANE_ENABLE);
        assert_eq!(state.to_state_bytes()[1], 1);
        let saved = state.clone();

        assert_eq!(read_status(), 0xa5);
        assert_eq!(state.to_state_bytes()[1], 0);
        assert_eq!(saved.to_state_bytes()[1], 1);
        state.clone_from(&saved);
        assert_eq!(state.to_state_bytes()[1], 1);

        // A retained status reader still resets the restored register bank.
        assert_eq!(read_status(), 0xa5);
        state.write(HORIZONTAL_PANNING);
        assert_eq!(state.address(), HORIZONTAL_PANNING);
        assert_eq!(state.values, saved.values);
        assert_eq!(saved.to_state_bytes()[1], 1);
    }
}
