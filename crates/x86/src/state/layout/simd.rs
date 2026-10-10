//! Literal SSE backing shared by generated code and host snapshots.

use super::codec::{read, read_u32, write};

/// Eight XMM encodings and the packed architectural MXCSR word.
/// XMM bytes are in little-endian order. This is not an FXSAVE image.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoredSimd {
    pub mxcsr: u32,
    /// Snapshot padding, retained without interpretation.
    pub reserved: [u8; 4],
    pub xmm: [[u8; 16]; 8],
}

impl Default for StoredSimd {
    fn default() -> Self {
        Self {
            mxcsr: Self::MXCSR_RESET,
            reserved: [0; 4],
            xmm: [[0; 16]; 8],
        }
    }
}

impl StoredSimd {
    /// Reset control: all exceptions masked, round to nearest, gradual underflow.
    pub const MXCSR_RESET: u32 = 0x1f80;

    /// Architecturally writable bits. This target supports DAZ (bit 6).
    pub const MXCSR_MASK: u32 = 0x0000_ffff;

    pub(super) fn read(bytes: &[u8]) -> Self {
        Self {
            mxcsr: read_u32(bytes, 0),
            reserved: read(bytes, 4),
            xmm: std::array::from_fn(|index| read(bytes, 8 + index * 16)),
        }
    }

    pub(super) fn write(&self, bytes: &mut [u8]) {
        write(bytes, 0, &self.mxcsr.to_le_bytes());
        write(bytes, 4, &self.reserved);
        for (index, value) in self.xmm.iter().enumerate() {
            write(bytes, 8 + index * 16, value);
        }
    }
}
