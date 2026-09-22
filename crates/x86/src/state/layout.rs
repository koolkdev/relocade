//! CPU backing fields shared by generated code and host state snapshots.

mod codec;
mod segments;
mod x87;

pub use segments::{Segments, StoredSegment};
pub use x87::{StoredX87, StoredX87Register};

#[cfg(test)]
mod tests;

use std::{
    mem::size_of,
    ops::{Index, IndexMut},
};

use crate::register::Gpr32;

/// Backing descriptor for the six arithmetic status flags.
/// Kind zero reads their stored bytes. Other supported kinds derive their values
/// from the payload; control and system flags always use their stored bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoredStatusSource {
    /// Zero selects stored bits. Subtraction uses 1/5/9, addition 2/6/10,
    /// and logic 3/7/11 for byte/word/dword values, respectively.
    pub kind: u8,
    /// Snapshot padding, retained without interpretation.
    pub reserved: [u8; 3],
    /// Left arithmetic operand or logical result; unused for kind zero.
    pub left: u32,
    /// Right arithmetic operand; unused for kind zero and logic.
    pub right: u32,
}

/// Named backing bytes for the represented x86 flags, in snapshot order.
/// Each flag uses its low bit. Status bytes can be stale while a status source
/// is active; snapshots retain every byte without interpreting or normalizing it.
/// This byte record is not the architectural EFLAGS bit encoding.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FlagBytes {
    pub cf: u8,
    pub pf: u8,
    pub af: u8,
    pub zf: u8,
    pub sf: u8,
    pub of: u8,
    pub tf: u8,
    pub df: u8,
    pub nt: u8,
    pub ac: u8,
    pub id: u8,
    /// Snapshot padding, not an architectural flag.
    pub reserved: u8,
}

/// Flag backing state, including inactive source operands and stale flag bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoredFlags {
    pub status_source: StoredStatusSource,
    pub bytes: FlagBytes,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Registers {
    pub eax: u32,
    pub ecx: u32,
    pub edx: u32,
    pub ebx: u32,
    pub esp: u32,
    pub ebp: u32,
    pub esi: u32,
    pub edi: u32,
}

impl Index<Gpr32> for Registers {
    type Output = u32;

    fn index(&self, register: Gpr32) -> &Self::Output {
        match register {
            Gpr32::Eax => &self.eax,
            Gpr32::Ecx => &self.ecx,
            Gpr32::Edx => &self.edx,
            Gpr32::Ebx => &self.ebx,
            Gpr32::Esp => &self.esp,
            Gpr32::Ebp => &self.ebp,
            Gpr32::Esi => &self.esi,
            Gpr32::Edi => &self.edi,
        }
    }
}

impl IndexMut<Gpr32> for Registers {
    fn index_mut(&mut self, register: Gpr32) -> &mut Self::Output {
        match register {
            Gpr32::Eax => &mut self.eax,
            Gpr32::Ecx => &mut self.ecx,
            Gpr32::Edx => &mut self.edx,
            Gpr32::Ebx => &mut self.ebx,
            Gpr32::Esp => &mut self.esp,
            Gpr32::Ebp => &mut self.ebp,
            Gpr32::Esi => &mut self.esi,
            Gpr32::Edi => &mut self.edi,
        }
    }
}

/// CPU backing state. Byte conversion is explicitly little endian on every host.
/// `Default` installs flat segment caches and an initialized x87 environment;
/// `filled` and `from_bytes` preserve literal backing images without initialization.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuState {
    pub flags: StoredFlags,
    pub registers: Registers,
    pub eip: u32,
    pub segments: Segments,
    pub reserved: [u8; 12],
    pub instruction_count: u32,
    pub reserved_tail: [u8; 4],
    pub x87: StoredX87,
}

impl CpuState {
    pub const BYTE_LEN: usize = size_of::<Self>();
}

impl Default for CpuState {
    fn default() -> Self {
        Self {
            segments: Segments::flat32(),
            x87: StoredX87::default(),
            ..Self::filled(0)
        }
    }
}
